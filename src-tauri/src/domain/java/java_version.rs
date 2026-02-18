#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JavaRuntime {
    Java8,
    Java17,
    Java21,
}

impl JavaRuntime {
    pub fn as_dir_name(self) -> &'static str {
        match self {
            JavaRuntime::Java8 => "java8",
            JavaRuntime::Java17 => "java17",
            JavaRuntime::Java21 => "java21",
        }
    }

    pub fn major(self) -> u8 {
        match self {
            JavaRuntime::Java8 => 8,
            JavaRuntime::Java17 => 17,
            JavaRuntime::Java21 => 21,
        }
    }
}
