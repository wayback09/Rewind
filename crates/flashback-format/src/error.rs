use std::fmt;

#[derive(Debug)]
pub struct FormatError {
    pub message: String,
    pub offset: Option<usize>,
    pub context: Option<String>,
}

impl FormatError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            offset: None,
            context: None,
        }
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(off) = self.offset {
            write!(f, "{} at offset {} (0x{:X})", self.message, off, off)?;
        } else {
            write!(f, "{}", self.message)?;
        }
        if let Some(ctx) = &self.context {
            write!(f, " [{}]", ctx)?;
        }
        Ok(())
    }
}

impl std::error::Error for FormatError {}

pub type Result<T> = std::result::Result<T, FormatError>;
