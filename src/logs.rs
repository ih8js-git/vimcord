use crate::Error;
use std::{fs::File, io::Write, path::PathBuf};

pub enum LogType {
    Error,
    #[allow(dead_code)]
    Warning,
    #[allow(dead_code)]
    Info,
    #[allow(dead_code)]
    Debug,
}

fn write_log_file(path: PathBuf, msg: &[u8]) -> Result<(), Error> {
    File::options()
        .append(true)
        .create(true)
        .open(path)?
        .write_all(msg)?;
    Ok(())
}

pub fn print_log(msg: Error, log_type: LogType) -> Result<(), Error> {
    let timestamp = chrono::offset::Local::now();
    let type_str = match log_type {
        LogType::Error => "ERROR",
        LogType::Warning => "WARN",
        LogType::Info => "INFO",
        LogType::Debug => "DEBUG",
    };
    let msg = format!("[{timestamp}] {type_str}: {msg}\n");
    let mut path = dirs::config_dir().unwrap_or(".".into());
    path.push("vimcord");
    path.push("logs");

    write_log_file(path, msg.as_bytes())?;
    Ok(())
}
