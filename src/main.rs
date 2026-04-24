use std::io::{self, Write};

fn main() -> io::Result<()> {
    print!("cawir> ");
    io::stdout().flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;

    println!("you said: {}", line.trim());

    Ok(())
}
