use ratatui::{
    Frame,
    backend::{Backend, ClearType},
};

/// The frame operations required by the event loop. Ratatui terminals can be owned or borrowed.
pub trait RenderTarget {
    type Error: std::error::Error;

    /// Draws one frame.
    fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> Result<(), Self::Error>;

    /// Invalidates the previous frame so the next draw repaints the whole screen.
    fn force_redraw(&mut self) -> Result<(), Self::Error>;
}

impl<B: Backend> RenderTarget for ratatui::Terminal<B> {
    type Error = B::Error;

    fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> Result<(), Self::Error> {
        ratatui::Terminal::draw(self, render).map(|_| ())
    }

    fn force_redraw(&mut self) -> Result<(), Self::Error> {
        // Terminal::clear queries the cursor position, which can block on a real TTY.
        self.backend_mut().clear_region(ClearType::All)?;
        self.swap_buffers();
        Ok(())
    }
}

impl<T: RenderTarget> RenderTarget for &mut T {
    type Error = T::Error;

    fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> Result<(), Self::Error> {
        (**self).draw(render)
    }

    fn force_redraw(&mut self) -> Result<(), Self::Error> {
        (**self).force_redraw()
    }
}
