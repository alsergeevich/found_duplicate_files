#![allow(non_snake_case)]
use std::{fs::OpenOptions, io::{BufReader, Read}, path::Path};

use md5::{Md5, Digest};

pub fn partial_hash_file<P>(path: P) -> Result<String, std::io::Error> 
    where P: AsRef<Path>
    {
        let mut hasher = Md5::new(); // Создаем новый экземпляр хешера
        let mut count64KBlocks = 0;
        let path = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .open(path)?;
        let mut reader = BufReader::new(file); // Создаем буферизованный ридер
        let mut buffer = [0; 65536];
        loop {
            let bytes_read = reader.read(&mut buffer)?; // Читаем файл блоками по 65536 байта
            if bytes_read == 0 {
                break; // Обязательно проверяем, вдруг файл закончился!
            }
            
            hasher.update(&buffer[..bytes_read]); // Обновляем хеш сначала
            
            count64KBlocks += 1;
            if count64KBlocks == 3 {
                break; // Если прочитали 3 блока (примерно 196 КБ) — прерываем чтение делаем это для ускорения работы программы так как в большинстве случаев файлы не совпадают
            }
        }

        let hash = hasher.finalize(); // Получаем хеш
        Ok(format!("{:?}", hash)) // Возвращаем хеш в виде строки

    }


pub fn full_hash_file<P>(path: P) -> Result<String, std::io::Error> 
    where P: AsRef<Path>
    {
        let mut hasher = Md5::new(); // Создаем новый экземпляр хешера
        let path = path.as_ref();
        let file = OpenOptions::new()
            .read(true)
            .open(path)?;
        let mut reader = BufReader::new(file); // Создаем буферизованный ридер
        let mut buffer = [0; 65536];
        loop {
            let bytes_read = reader.read(&mut buffer)?; // Читаем файл блоками по 65536 байта
            if bytes_read == 0 {
                break; // Обязательно проверяем, вдруг файл закончился!
            }
            
            hasher.update(&buffer[..bytes_read]); // Обновляем хеш сначала
            
        }

        let hash = hasher.finalize(); // Получаем хеш
        Ok(format!("{:?}", hash)) // Возвращаем хеш в виде строки

    }