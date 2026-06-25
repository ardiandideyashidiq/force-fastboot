use owo_colors::OwoColorize;

pub fn error(msg: impl AsRef<str>) -> String {
    msg.as_ref().red().bold().to_string()
}

pub fn warn(msg: impl AsRef<str>) -> String {
    msg.as_ref().yellow().to_string()
}

pub fn ok(msg: impl AsRef<str>) -> String {
    msg.as_ref().green().to_string()
}

pub fn dim(msg: impl AsRef<str>) -> String {
    msg.as_ref().dimmed().to_string()
}

pub fn heading(msg: impl AsRef<str>) -> String {
    msg.as_ref().white().bold().to_string()
}

pub fn info(msg: impl AsRef<str>) -> String {
    msg.as_ref().bright_blue().to_string()
}
