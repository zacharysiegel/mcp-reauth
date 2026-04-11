use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::{self, Debug, Display, Formatter};
use std::{error, io};

macro_rules! impl_from_error {
    ($error_type:ty) => {
        impl From<$error_type> for Error {
            fn from(value: $error_type) -> Self {
                Self::from_error_default(Box::new(value))
            }
        }
    };
}

pub struct Error {
    pub message: String,
    pub sub_error: Option<Box<dyn error::Error>>,
    pub backtrace: Backtrace,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Error [{}]", self.message)?;
        if let Some(sub_error) = &self.sub_error {
            write!(f, "\n[{}]", sub_error)?;
        }
        match self.backtrace.status() {
            BacktraceStatus::Captured => write!(f, "\n{}", self.backtrace),
            _ => Ok(()),
        }
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl error::Error for Error {}

impl Error {
    pub fn new(message: &str) -> Error {
        Self::_new(message, None)
    }

    pub fn from_error_default(error: Box<dyn error::Error>) -> Error {
        Self::_new(&error.to_string(), Some(error))
    }

    fn _new(message: &str, error: Option<Box<dyn error::Error>>) -> Error {
        Error {
            message: message.to_string(),
            sub_error: error,
            backtrace: Backtrace::force_capture(),
        }
    }
}

impl_from_error!(io::Error);
impl_from_error!(serde_json::Error);
impl_from_error!(ureq::Error);
impl_from_error!(url::ParseError);

