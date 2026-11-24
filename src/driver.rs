use std::{
    collections::VecDeque,
    future::Future,
    task::{Context, Poll},
};

use crate::Components;
use futures_util::{
    FutureExt as _, Stream, StreamExt as _,
    future::{AbortHandle, BoxFuture},
    stream::{self, BoxStream, FuturesUnordered, SelectAll},
};
use tokio::sync::mpsc;

/// Interprets the operations produced by a compositor. A driver can own API
/// clients, caches and other application-specific resources.
pub trait Driver<A: Components> {
    /// Stops resources owned by the driver when the application finishes.
    fn shutdown(&mut self) {}

    fn dispatch(&mut self, operation: A::Operation, cx: &mut DriverContext<A::Event>);

    /// Polls sources owned by this driver, such as a credential renewal timer.
    /// Return pending until the source can provide another event.
    fn poll_event(&mut self, _cx: &mut Context<'_>) -> Poll<A::Event> {
        Poll::Pending
    }
}

/// Identifies a request for the lifetime of a running application loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    /// Constructs an ID for replaying or testing application events.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Lets one request publish progress with the same ID as its start and completion.
pub struct RequestContext<E> {
    id: RequestId,
    sender: mpsc::UnboundedSender<E>,
}

impl<E> RequestContext<E> {
    pub fn id(&self) -> RequestId {
        self.id
    }

    /// Returns the event if the application loop has already exited.
    pub fn emit(
        &self,
        event: impl FnOnce(RequestId) -> E,
    ) -> Result<(), mpsc::error::SendError<E>> {
        self.sender.send(event(self.id))
    }
}

/// Owns the event queue and concurrently polled futures and streams of one run.
pub struct DriverContext<E> {
    jobs: FuturesUnordered<BoxFuture<'static, E>>,
    watches: SelectAll<BoxStream<'static, E>>,
    ready: VecDeque<E>,
    sender: mpsc::UnboundedSender<E>,
    receiver: mpsc::UnboundedReceiver<E>,
    next_request_id: u64,
    redraw: bool,
}

impl<E> DriverContext<E> {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            jobs: FuturesUnordered::new(),
            watches: SelectAll::new(),
            ready: VecDeque::new(),
            sender,
            receiver,
            next_request_id: 0,
            redraw: false,
        }
    }

    /// Queues a result from a synchronous operation.
    pub fn emit(&self, event: E) {
        let _ = self.sender.send(event);
    }

    /// Schedules an independent operation and delivers its result to the UI.
    pub fn submit(&mut self, future: impl Future<Output = E> + Send + 'static) {
        self.jobs.push(future.boxed());
    }

    /// Queues a start event and schedules the matching completion. The future
    /// may use the supplied context to publish intermediate results.
    pub fn request<F>(&mut self, build: impl FnOnce(RequestContext<E>) -> (E, F))
    where
        F: Future<Output = E> + Send + 'static,
    {
        let id = RequestId(self.next_request_id);
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .expect("request IDs exhausted");
        // Reserve the start event's place ahead of any progress emitted by build.
        while let Ok(event) = self.receiver.try_recv() {
            self.ready.push_back(event);
        }
        let (started, future) = build(RequestContext {
            id,
            sender: self.sender.clone(),
        });
        self.ready.push_back(started);
        self.submit(future);
    }

    /// Subscribes to an event stream. Use the handle to stop a watch early.
    pub fn watch(&mut self, events: impl Stream<Item = E> + Send + 'static) -> AbortHandle {
        let (events, handle) = stream::abortable(events);
        self.watches.push(events.boxed());
        handle
    }

    /// Repaints the entire fullscreen terminal after an external program has used it.
    pub fn force_redraw(&mut self) {
        self.redraw = true;
    }

    pub(crate) fn take_redraw_request(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.jobs.is_empty() || !self.ready.is_empty() || !self.receiver.is_empty()
    }

    pub(crate) async fn next_event(&mut self) -> E {
        if let Some(event) = self.ready.pop_front() {
            return event;
        }
        loop {
            if let Ok(event) = self.receiver.try_recv() {
                return event;
            }
            let result = tokio::select! {
                Some(event) = self.receiver.recv() => return event,
                result = self.jobs.next(), if !self.jobs.is_empty() => result,
                result = self.watches.next(), if !self.watches.is_empty() => result,
            };
            if let Some(event) = result {
                // Put completion after any progress published while polling the job.
                self.emit(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use tokio::sync::oneshot;

    #[derive(Debug, PartialEq)]
    enum Event {
        Start(RequestId),
        Progress(RequestId),
        Done(RequestId),
        Other,
    }

    #[tokio::test]
    async fn requests_preserve_progress_order_while_other_work_can_finish() {
        let mut cx = DriverContext::new();
        let (resume, wait) = oneshot::channel();
        cx.request(|request| {
            let id = request.id();
            (Event::Start(id), async move {
                wait.await.unwrap();
                request.emit(Event::Progress).unwrap();
                Event::Done(id)
            })
        });
        let Event::Start(first) = cx.next_event().await else {
            panic!("expected request start")
        };
        cx.request(|request| {
            let id = request.id();
            (Event::Start(id), async move { Event::Done(id) })
        });
        let Event::Start(second) = cx.next_event().await else {
            panic!("expected second start")
        };
        assert_ne!(first, second);
        assert_eq!(cx.next_event().await, Event::Done(second));
        resume.send(()).unwrap();
        assert_eq!(cx.next_event().await, Event::Progress(first));
        assert_eq!(cx.next_event().await, Event::Done(first));
        assert!(cx.next_event().now_or_never().is_none());
    }

    #[tokio::test]
    async fn progress_emitted_while_building_a_request_follows_its_start() {
        let mut cx = DriverContext::new();
        cx.emit(Event::Other);
        cx.request(|request| {
            request.emit(Event::Progress).unwrap();
            (Event::Start(request.id()), async move {
                Event::Done(request.id())
            })
        });
        assert_eq!(cx.next_event().await, Event::Other);
        let Event::Start(id) = cx.next_event().await else {
            panic!("expected request start")
        };
        assert_eq!(cx.next_event().await, Event::Progress(id));
        assert_eq!(cx.next_event().await, Event::Done(id));
    }

    #[tokio::test]
    async fn cancelling_a_watch_leaves_the_queue_usable() {
        let mut cx = DriverContext::new();
        let watch = cx.watch(stream::repeat_with(|| Event::Other));
        assert_eq!(cx.next_event().await, Event::Other);
        watch.abort();
        assert!(cx.next_event().now_or_never().is_none());
        cx.submit(async { Event::Other });
        assert_eq!(cx.next_event().await, Event::Other);
    }
}
