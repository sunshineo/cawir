use std::io::{self, Write};

fn main() -> io::Result<()> {
    loop {
        print!("cawir> ");
        io::stdout().flush()?;

        let mut line = String::new();
        let bytes_read = io::stdin().read_line(&mut line)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        println!("you said: {}", line.trim());
    }

    Ok(())
}
