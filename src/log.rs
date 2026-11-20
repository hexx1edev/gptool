use colored::Colorize;

pub fn status(action: &str, message: &str) {
    let padded = format!("{:>12}", action);
    println!("{} {}", padded.green().bold(), message);
}

pub fn warning(message: &str) {
    let padded = format!("{:>12}", "Warning");
    println!("{} {}", padded.yellow().bold(), message);
}

pub fn error(message: &str) {
    let padded = format!("{:>12}", "Error");
    eprintln!("{} {}", padded.red().bold(), message);
}

pub fn info(action: &str, message: &str) {
    let padded = format!("{:>12}", action);
    println!("{} {}", padded.cyan().bold(), message);
}
