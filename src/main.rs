#![allow(unused)]

use std::path::PathBuf;
use std::collections::{HashMap, VecDeque};

mod md5;
mod group;


fn main() -> Result<(), std::io::Error> {
    
    // Собираем все аргументы из терминала и склеиваем их пробелом.
    // Это спасет, если пользователь передал путь с пробелами без кавычек в терминале.
    let collected_args: Vec<String> = std::env::args().skip(1).collect();
    if collected_args.is_empty() {
        eprintln!("Пожалуйста, укажите путь к директории!");
        std::process::exit(1);
    }
    // Убираем случайные кавычки и пробелы с краев — иногда при копировании 
    // или автодополнении они могут стать частью строки и ломать путь к файлу.
    let path_dir = collected_args.join(" ").trim_matches(|c| c == '\"' || c == '\'' || c == ' ').to_string();
    let path = PathBuf::from(&path_dir); //преобразуем путь в PathBuf
    // println!("Путь к директории (сырой вид): {:?}", path);

    let vec_files = group::directory_traversal(path)?;
    println!("Будет обработано файлов: {}", vec_files.len());

    //теперь у нас есть все файлы в векторе vec_files
    //теперь группируем файлы по размеру
    let map_files_by_size = group::group_files_by_size(vec_files)?;

    //теперь для каждой группы файлов с одинаковым размером вычисляем частичный md5 хеш
    let partial_hash_to_found_duplicate = group::group_files_by_partial_hash(map_files_by_size)?;

    //теперь для каждой группы файлов с одинаковым частичным md5 хеш вычисляем полный md5 хеш
    let full_hash_to_found_duplicate = group::group_files_by_full_hash(partial_hash_to_found_duplicate)?;
    
    println!("Найдено дубликатов: {}", full_hash_to_found_duplicate.len());

    //выводим результат
    for (hash, paths) in full_hash_to_found_duplicate {
        println!("Дубликаты, MD5: {}", hash);
        for path in paths {
            println!("  {}", path.display());
        }
        println!();
    }
    
    Ok(())
}