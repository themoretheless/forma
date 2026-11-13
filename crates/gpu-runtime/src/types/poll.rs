//! Submission indices and polling.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubmissionIndex(pub u64);

#[derive(Debug, Clone)]
pub enum PollType {
    Poll,
    Wait {
        submission_index: Option<SubmissionIndex>,
        timeout: Option<std::time::Duration>,
    },
}

impl PollType {
    pub fn wait_indefinitely() -> Self {
        Self::Wait {
            submission_index: None,
            timeout: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollStatus {
    QueueEmpty,
    WaitSucceeded,
    Poll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    Timeout,
    WrongSubmissionIndex(SubmissionIndex, SubmissionIndex),
}

impl std::fmt::Display for PollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Poll wait timed out"),
            Self::WrongSubmissionIndex(a, b) => {
                write!(f, "Wrong submission index: {:?} vs {:?}", a, b)
            }
        }
    }
}

impl std::error::Error for PollError {}
