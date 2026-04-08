use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use rayon::iter::{IntoParallelIterator, ParallelBridge};
use rayon::iter::ParallelIterator;
use crate::md5::*;
//функция нулевого шага, обходит все папки в дереве и собирает все файлы
pub fn directory_traversal(path: PathBuf) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut vec_dirs = VecDeque::new();
    let mut vec_files = Vec::new();

    //первый проход по начальному каталогу
    let read_dir = match path.read_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Ошибка чтения стартовой папки {:?} : {}", path, e);
            return Err(e);
        }
    };
    
    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            vec_dirs.push_back(path);
        } else {
            vec_files.push(path);
        }
    }

    //дальше идем по всем папкам в очереди пока она не опустеет и заполняем вектор файлов
    //таким образом обходим все папки в дереве
    while let Some(path) = vec_dirs.pop_front() { //пока очередь не пуста берем первый элемент
        let read_dir = match path.read_dir() { //обрабатываем ошибки чтения папки
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("Ошибка чтения папки {:?} : {}", path, e);
                continue; // пропускаем проблемную папку и идем дальше!
            }
        };
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                vec_dirs.push_back(path); //если папка то добавляем в очередь
            } else {
                vec_files.push(path); //если файл то добавляем в вектор файлов
            }
        }
    }
    
    Ok(vec_files)
}

//функция первого шага, группирует файлы по размеру
pub fn group_files_by_size(paths: Vec<PathBuf>) -> Result<HashMap<u64, Vec<PathBuf>>, std::io::Error> {
    // Создаем HashMap: ключ - размер в байтах (u64), 
    // значение - вектор с путями к файлам такого размера
    let mut map_files = HashMap::new();
    for path in paths {
        // Узнаем метаданные файла (в том числе его размер)
        let metadata = match fs::metadata(&path) { //пытаемся получить метаданные файла
            Ok(m) => m, //если получилось то присваиваем
            Err(e) => {
                eprintln!("Ошибка чтения размера у файла: {:?}. Код ошибки: {}", path, e);
                continue; // пропускаем битые файлы (например, сломанные ярлыки) и идем дальше!
            }
        };
        // Получаем размер файла
        let size = metadata.len();
        // Добавляем путь к файлу в HashMap
        map_files.entry(size).or_insert_with(Vec::new).push(path);
    }
    // for (size, paths) in map_files {
    //     if paths.len() > 1 {
    //         continue;
    //     } else {
    //         map_files.remove(&size);
    //     }
    // }
    map_files.retain(|_, paths| paths.len() > 1);  //удаляем все записи где только один файл
    


    Ok(map_files)
}

//функция второго шага, группирует файлы по частичному хешу
pub fn group_files_by_partial_hash(group_by_size: HashMap<u64, Vec<PathBuf>>) -> Result<HashMap<(u64, String), Vec<PathBuf>>, std::io::Error> {
    let mut map_files_by_hash: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new(); //для более точного сравнения файлов добавляем размер в ключ
    for (size, paths) in group_by_size {
        for path in paths {
            match partial_hash_file(&path) {
                Ok(hash) => {
                    map_files_by_hash
                        .entry((size, hash))
                        .or_insert_with(Vec::new)
                        .push(path);
                }
                Err(e) => {
                    eprintln!("Ошибка вычисления хеша для файла {:?} : {}", path, e);
                }
            }
        }
    }
    Ok(map_files_by_hash)
}
//функция третьего шага, группирует файлы по полному хешу
// pub fn group_files_by_full_hash(group_by_partial_hash: HashMap<(u64, String), Vec<PathBuf>>) -> Result<HashMap<String, Vec<PathBuf>>, std::io::Error> {
//     let all_files: Vec<PathBuf> = group_by_partial_hash.into_values().flat_map(|paths| paths.into_iter()).collect(); //проходим по всей group_by_partial_hash и собираем все файлы в один вектор
//     let mut map_files_by_hash = Mutex::new(HashMap::new()); //тут можно обойтись без size, оборачиваем в мютекс
    
//     all_files.into_par_iter()
//         .for_each(|path| {
//             match full_hash_file(&path) {
//                 Ok(hash) => {
//                     map_files_by_hash.lock().unwrap().entry(hash).or_insert_with(Vec::new).push(path);
//                     }
//                     Err(e) => {
//                         eprintln!("Ошибка вычисления хеша для файла {:?} : {}", path, e);
//                     }
//                 }
//         });
//     let mut map_files_by_hash = map_files_by_hash.into_inner().unwrap();
//     map_files_by_hash.retain(|_, paths| paths.len() > 1);  //удаляем все записи где только один файл
//     Ok(map_files_by_hash)
// }
pub fn group_files_by_full_hash(mut group_by_partial_hash: HashMap<(u64, String), Vec<PathBuf>>) -> Result<HashMap<String, Vec<PathBuf>>, std::io::Error> {
    //после того как мы сгруппировали файлы по частичному хешу, проверим есть ли среди них одиночные файлы и отсеем их
    group_by_partial_hash.retain(|_, paths| paths.len() > 1); //удаляем все записи где только один файл
    //используем вместо mutex map-reduce (fold/reduce)
    let final_map = group_by_partial_hash
        .into_values()
        .flatten()
        .par_bridge()
        .fold(|| HashMap::<String, Vec<PathBuf>>::new(), |mut acc: HashMap<String, Vec<PathBuf>>, path: PathBuf| {
            if let Ok(hash) = full_hash_file(&path) {
                acc.entry(hash).or_default().push(path);
            }
            acc
        },
        )
        .reduce(|| HashMap::new(), |mut map1: HashMap<String, Vec<PathBuf>>, map2: HashMap<String, Vec<PathBuf>>| {   //объединяем карты
            for (hash, paths) in map2 {
                map1.entry(hash).or_default().extend(paths);
            }
            map1
        });
        let mut result = final_map;
        result.retain(|_, paths: &mut Vec<PathBuf>| paths.len() > 1);  //удаляем все записи где только один файл
        Ok(result)


    
    
    
}
