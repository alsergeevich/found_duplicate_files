pub fn parse_args() -> Result<PathBuf, std::io::Error> {
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
    Ok(path)
}