use std::{fs::OpenOptions, io::{BufRead, BufReader}, path::Path};






pub fn counter_words<P>(path: P) -> Result<u32, std::io::Error> 
    where P : AsRef<Path> 
    {
        let path = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .open(path)?;

        let reader = BufReader::new(file);
        let mut count = 0;

        for line in reader.lines() {
            count += line?.split_whitespace().count() as u32;
        }
        Ok(count)
    
    }