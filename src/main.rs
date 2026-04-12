#![allow(unused)]


// io: ввод-вывод для работы с терминалом и очистки экрана
// path: работа с системными путями (преобразование в строку, проверка существования)
// collections: HashMap используется для хранения результатов поиска
// time: измерение длительности выполнения операций (Instant, Duration)
// mpsc: механизм каналов для асинхронного общения между потоками (Sender/Receiver)
// thread: запуск фоновых задач, чтобы интерфейс не зависал при сканировании диска
use std::io::{self, Error, Write};
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;


// crossterm: управление событиями клавиатуры и режимом терминала
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
// ratatui: фреймворк для создания графического интерфейса в терминале
use ratatui::{
    buffer::Buffer,
    layout::{Layout, Constraint, Rect, Direction, Alignment, Margin},
    style::{Color, Modifier, Style, Stylize, palette::tailwind},
    symbols::{self, border},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, List, ListItem, ListState, Clear, Padding, Gauge, BorderType},
    DefaultTerminal, Frame,
};
// rfd: вызов нативного окна выбора папки в операционной системе
use rfd::FileDialog;

// === МОДУЛИ ПРОЕКТА (бизнес-логика хеширования и обхода файлов) ===
mod md5;
mod group;


/// Перечисление типов сообщений, которые фоновый поток сканирования может отправить в основной TUI
enum ScanMessage {
    /// Сообщение о текущем файле (для создания эффекта "живого" прогресса)
    Progress(String),      
    /// Сообщение об успешном завершении со структурой результатов
    Finished(SearchResults), 
    /// Сообщение о критической ошибке в процессе поиска
    Error(String),          
}

/// Структура для хранения итоговых данных поиска
struct SearchResults {
    /// Общее кол-во файлов, которые пришлось просмотреть
    total_files: usize,
    /// Сгруппированные дубликаты: (MD5-хэш, Вектор путей к файлам)
    duplicates: Vec<(String, Vec<PathBuf>)>, 
    /// Время, которое ушло на все этапы (от обхода до полного хеширования)
    duration: Duration,
}

/// Перечисление возможных состояний приложения (стадий жизненного цикла)
enum AppStatus {
    /// Приложение запущено, папка еще не выбрана
    Idle,            
    /// Папка выбрана и проверена, можно запускать поиск
    Ready,           
    /// Фоновый поток активно читает диск и считает хеши
    Searching,       
    /// Поиск окончен, на экране отображается список дубликатов
    ResultsAvailable, 
}

/// ОСНОВНАЯ СТРУКТУРА ПРИЛОЖЕНИЯ
/// Хранит всё состояние TUI, необходимое для отрисовки в каждый момент времени
struct App {
    /// Путь к папке, выбранной пользователем через диалог
    selected_path: Option<PathBuf>,
    /// Контент с результатами (появляется после завершения поиска)
    results: Option<SearchResults>,
    /// Текущее логическое состояние приложения
    status: AppStatus,
    /// Текстовая строка статуса в нижней части экрана (информирует пользователя)
    status_message: String,
    /// Состояние виджета List (хранит индекс выделенной строки для прокрутки)
    list_state: ListState,
    /// Флаг выхода: когда станет true, основной цикл завершится
    should_quit: bool,
    /// Приемник сообщений от канала (None, если поиск не активен)
    rx: Option<Receiver<ScanMessage>>,
    /// Имя файла, который обрабатывается прямо сейчас (для отображения в UI)
    current_scanning_file: String,
}

impl App {
    /// Создает новый экземпляр приложения с начальными ("чистыми") значениями
    fn new() -> Self {
        Self {
            selected_path: None,
            results: None,
            status: AppStatus::Idle,
            status_message: "Добро пожаловать в Duplicate Finder!".to_string(),
            list_state: ListState::default(),
            should_quit: false,
            rx: None,
            current_scanning_file: String::new(),
        }
    }

    /// ГЛАВНЫЙ ЦИКЛ ПРИЛОЖЕНИЯ (вызывается из main)
    pub fn run(&mut self, mut terminal: DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            // 1. Отрисовка: передаем текущее состояние 'self' в функцию draw
            terminal.draw(|frame| self.draw(frame))?;

            // 2. Обработка асинхронных сообщений (если идет поиск)
            let mut should_clear_rx = false;
            // Пытаемся заглянуть в канал 'rx', если он существует
            if let Some(ref receiver) = self.rx {
                // Вычитываем ВСЕ накопленные сообщения из канала без блокировки основного потока
                while let Ok(msg) = receiver.try_recv() {
                    match msg {
                        // Обновляем текст текущего файла в UI
                        ScanMessage::Progress(file) => {
                            self.current_scanning_file = file;
                        }
                        // Фиксируем результат и переходим в режим просмотра
                        ScanMessage::Finished(res) => {
                            self.results = Some(res);
                            self.status = AppStatus::ResultsAvailable;
                            self.status_message = "Готово!".to_string();
                            self.list_state.select(Some(0)); // Выделяем первую группу
                            should_clear_rx = true; // Отмечаем, что канал пора закрыть
                        }
                        // Выводим ошибку, если что-то пошло не так
                        ScanMessage::Error(e) => {
                            self.status_message = format!("Ошибка: {}", e);
                            self.status = AppStatus::Ready;
                            should_clear_rx = true;
                        }
                    }
                }
            }
            // Удаляем канал из состояния, когда поток закончил работу
            if should_clear_rx {
                self.rx = None;
            }

            // 3. Обработка пользовательского ввода (событий клавиатуры)
            // Ждем 16мс (соответствует 60 кадрам в секунду), чтобы интерфейс реагировал мгновенно
            if event::poll(Duration::from_millis(16))? {
                if let Event::Key(key) = event::read()? {
                    // Обрабатываем только нажатие (а не отпускание или повтор)
                    if key.kind == KeyEventKind::Press {
                        self.handle_key_event(key);
                    }
                }
            }
        }
        Ok(())
    }

    /// Обработка кнопок управления
    fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            // Выход из программы
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            // Выбор папки (английская 'd' или русская 'в')
            KeyCode::Char('d') | KeyCode::Char('в') => self.select_directory(),
            // Пуск поиска (английская 's' или русская 'ы')
            KeyCode::Char('s') | KeyCode::Char('ы') => self.start_async_search(),
            // Очистка всех данных (английская 'c' или русская 'с')
            KeyCode::Char('c') | KeyCode::Char('с') => self.clear_results(),
            // Навигация по списку (Стрелки или H-J-K-L маппинг)
            KeyCode::Down | KeyCode::Char('j') => self.next_item(),
            KeyCode::Up | KeyCode::Char('k') => self.previous_item(),
            _ => {}
        }
    }

    /// ОТКРЫТИЕ ДИАЛОГА ВЫБОРА ДИРЕКТОРИИ
    fn select_directory(&mut self) {
        // Не даем менять папку, если прямо сейчас идет сканирование
        if matches!(self.status, AppStatus::Searching) { return; }
        
        // rfd вызывает привычное окно ОС (Explorer в Win, Finder в Mac, GTK/KDE в Linux)
        if let Some(path) = FileDialog::new().pick_folder() {
            self.selected_path = Some(path.clone());
            self.status = AppStatus::Ready;
            self.status_message = format!("Выбрано: {}", path.display());
            self.results = None; // Сбрасываем старые результаты при выборе новой папки
        }
    }

    /// ЗАПУСК ФОНОВОГО ПРОЦЕССА ПОИСКА (БЕЗ ФРИЗОВ)
    fn start_async_search(&mut self) {
        // Игнорируем нажатие, если поиск уже идет
        if matches!(self.status, AppStatus::Searching) { return; }
        
        let path = match &self.selected_path {
            Some(p) => p.clone(),
            None => return, // Если папка не выбрана, ничего не делаем
        };

        // Создаем канал для связи потоков: tx - передатчик (в потоке), rx - приемник (в TUI)
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = AppStatus::Searching;
        self.status_message = "Идет поиск дубликатов...".to_string();

        // Запускаем новый поток ОС для тяжелых вычислений
        thread::spawn(move || {
            let start_time = Instant::now();
            
            // Замыкание для последовательного выполнения логики из наших модулей
            let res = (|| -> Result<SearchResults, Box<dyn std::error::Error + Send + Sync>> {
                // Шаг 0: Получаем все файлы в директории
                let vec_files = group::directory_traversal(path)?;
                let total_files = vec_files.len();

                // Шаг 1: Группируем по размеру (файлы одного размера потенциально дубликаты)
                let map_files_by_size = group::group_files_by_size(vec_files)?;
                
                // Шаг 2: Считаем частичный хеш и финальный MD5 для подтверждения
                let partial_hash = group::group_files_by_partial_hash(map_files_by_size)?;
                let full_hash = group::group_files_by_full_hash(partial_hash)?;

                // Преобразуем хеш-карту в вектор для удобного отображения по индексу в UI
                let mut duplicates: Vec<(String, Vec<PathBuf>)> = full_hash.into_iter().collect();
                
                // Сортируем: самые тяжелые группы (по размеру первого файла) будут вверху
                duplicates.sort_by(|a, b| {
                    let size_a = a.1[0].metadata().map(|m| m.len()).unwrap_or(0);
                    let size_b = b.1[0].metadata().map(|m| m.len()).unwrap_or(0);
                    size_b.cmp(&size_a)
                });

                Ok(SearchResults {
                    total_files,
                    duplicates,
                    duration: start_time.elapsed(),
                })
            })();

            // Отправляем результат или ошибку обратно в основной поток через канал
            match res {
                Ok(results) => { tx.send(ScanMessage::Finished(results)).ok(); }
                Err(e) => { tx.send(ScanMessage::Error(e.to_string())).ok(); }
            }
        });
    }

    /// Сброс интерфейса в начальное состояние
    fn clear_results(&mut self) {
        if matches!(self.status, AppStatus::Searching) { return; }
        self.results = None;
        self.status = if self.selected_path.is_some() { AppStatus::Ready } else { AppStatus::Idle };
        self.status_message = "Данные очищены.".to_string();
    }

    /// Выбор СЛЕДУЮЩЕГО элемента в списке (БЕЗ зацикливания)
    fn next_item(&mut self) {
        if let Some(res) = &self.results {
            let i = match self.list_state.selected() {
                // Если мы уже на последнем элементе, оставляем индекс прежним
                Some(i) => if i >= res.duplicates.len() - 1 { i } else { i + 1 },
                None => 0,
            };
            self.list_state.select(Some(i));
        }
    }

    /// Выбор ПРЕДЫДУЩЕГО элемента в списке (БЕЗ зацикливания)
    fn previous_item(&mut self) {
        if let Some(res) = &self.results {
            let i = match self.list_state.selected() {
                // Если мы на первом (нулевом) элементе, остаемся на нем
                Some(i) => if i == 0 { 0 } else { i - 1 },
                None => 0,
            };
            self.list_state.select(Some(i));
        }
    }

    /// ГЛАВНАЯ ФУНКЦИЯ ОТРИСОВКИ КАЖДОГО КАДРА
    fn draw(&mut self, frame: &mut Frame) {
        // ОПРЕДЕЛЕНИЕ ЦВЕТОВОЙ ПАЛИТРЫ (стиль "Dark Mode")
        let bg_color = Color::Rgb(15, 23, 42); // Цвет фона (Slate 900)
        let panel_color = Color::Rgb(30, 41, 59); // Цвет панелей
        let accent_color = Color::Rgb(56, 189, 248); // Акцентный голубой
        
        let area = frame.area();
        
        // СОЗДАНИЕ МАКЕТА: делим всё пространство на 3 горизонтальных отсека
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Отсек 0: Шапка (3 строки)
                Constraint::Min(0),    // Отсек 1: Основной контент (всё остальное место)
                Constraint::Length(3), // Отсек 2: Подвал (3 строки)
            ])
            .split(area);

        // --- 1. ШАПКА (HEADER) ---
        let header_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray))
            .bg(bg_color);
        
        let title = Line::from(vec![
            Span::styled(" FS ", Style::default().bg(accent_color).fg(Color::Black).add_modifier(Modifier::BOLD)),
            Span::styled(" DUPLICATE FINDER PRO ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]);

        let help = Line::from(vec![
            Span::styled(" [D] ", Style::default().fg(accent_color)), Span::raw("Папка  "),
            Span::styled(" [S] ", Style::default().fg(tailwind::GREEN.c400)), Span::raw("Поиск  "),
            Span::styled(" [C] ", Style::default().fg(tailwind::RED.c400)), Span::raw("Очистить  "),
            Span::styled(" [Q] ", Style::default().fg(Color::Gray)), Span::raw("Выйти"),
        ]);

        // Рендерим заголовок слева и подсказки справа в одном блоке
        frame.render_widget(Paragraph::new(title).block(header_block.clone().padding(Padding::horizontal(2))), chunks[0]);
        frame.render_widget(Paragraph::new(help).alignment(Alignment::Right).block(header_block.padding(Padding::horizontal(2))), chunks[0]);

        // --- 2. ЦЕНТРАЛЬНАЯ ЧАСТЬ (BODY) ---
        match self.status {
            // Режим отображения процесса сканирования
            AppStatus::Searching => {
                let loading_area = centered_rect(60, 20, chunks[1]);
                let loading_block = Block::default()
                    .title(" ИДЕТ СКАНИРОВАНИЕ... ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(accent_color));
                
                let msg = format!("Пожалуйста, подождите...\n\n{}", self.current_scanning_file);
                frame.render_widget(Paragraph::new(msg).block(loading_block).alignment(Alignment::Center), loading_area);
            }
            // Режим просмотра результатов
            AppStatus::ResultsAvailable => {
                if let Some(res) = &self.results {
                    // Генерируем элементы списка ListItem для каждого набора дубликатов
                    let list_items: Vec<ListItem> = res.duplicates.iter().enumerate().map(|(idx, (hash, paths))| {
                        let mut lines = vec![
                            Line::from(vec![
                                Span::styled(format!("Группа #{} ", idx + 1), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                                Span::styled(format!("({} файла)", paths.len()), Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC)),
                            ])
                        ];
                        
                        // Добавляем пути к файлам под заголовком группы
                        for path in paths {
                            let path_str = path.to_string_lossy();
                            lines.push(Line::from(vec![
                                Span::raw("  "),
                                Span::styled("󰿶 ", Style::default().fg(accent_color)), // Иконка файла
                                Span::styled(path_str, Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)),
                            ]));
                        }
                        lines.push(Line::from("")); // Пустая строка-разделитель
                        
                        ListItem::new(lines)
                    }).collect();

                    let list = List::new(list_items)
                        .block(Block::default()
                            .borders(Borders::ALL)
                            .title(" РЕЗУЛЬТАТЫ ")
                            .border_type(BorderType::Thick)
                            .border_style(Style::default().fg(accent_color))
                            .padding(Padding::uniform(1)))
                        .highlight_style(Style::default().bg(panel_color).add_modifier(Modifier::BOLD))
                        .highlight_symbol("▶ "); // Символ выделенной группы

                    // Используем stateful_widget для сохранения положения прокрутки
                    frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
                }
            }
            // Режим ожидания (начальный экран)
            _ => {
                let welcome_area = centered_rect(50, 30, chunks[1]);
                let welcome_msg = if let Some(p) = &self.selected_path {
                    format!("ПАПКА ГОТОВА\n\n{}\n\nНажмите [S], чтобы начать поиск", p.display())
                } else {
                    "СКАНЕР ДУБЛИКАТОВ\n\nНажмите [D], чтобы выбрать рабочую директорию".to_string()
                };
                
                let welcome = Paragraph::new(welcome_msg)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .padding(Padding::vertical(2)))
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::Gray));
                frame.render_widget(welcome, welcome_area);
            }
        }

        // --- 3. НИЖНЯЯ ПАНЕЛЬ (FOOTER) ---
        let footer_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray))
            .bg(bg_color);

        // Формируем строку статистики: сколько групп найдено и время работы
        let status_text = if let Some(res) = &self.results {
            format!(" Найдено групп: {} | Обработано файлов: {} | Время: {:?} ", res.duplicates.len(), res.total_files, res.duration)
        } else {
            format!(" Статус: {} ", self.status_message)
        };

        let footer = Paragraph::new(status_text)
            .block(footer_block.padding(Padding::horizontal(2)))
            .fg(Color::DarkGray);
        
        frame.render_widget(footer, chunks[2]);
    }
}

/// ВСПОМОГАТЕЛЬНАЯ ФУНКЦИЯ ДЛЯ ЦЕНТРИРОВАНИЯ ЭЛЕМЕНТОВ (ВСПЛЫВАЮЩИХ ОКОН)
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    // Сначала делим вертикально
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    // Затем получившуюся середину делим горизонтально
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// === ТОЧКА ВХОДА В ПРОГРАММУ ===
fn main() -> Result<(), color_eyre::Report> {
    // Инициализация продвинутой обработки ошибок (выводит красивые стектрейсы)
    color_eyre::install()?;

    // Переводим терминал в режим TUI
    let mut terminal = ratatui::init();
    
    // ПРИНУДИТЕЛЬНАЯ ОЧИСТКА ТЕРМИНАЛА ПЕРЕД СТАРТОМ
    // Это гарантирует отсутствие артефактов от предыдущих команд в консоли
    io::stdout().write_all(b"\x1b[2J\x1b[H").unwrap(); 
    
    let mut app = App::new();
    
    // Запуск бесконечного цикла приложения
    let result = app.run(terminal);

    // ОБЯЗАТЕЛЬНОЕ ВОССТАНОВЛЕНИЕ: возвращаем терминал в обычный режим
    // Если этого не сделать, после выхода консоль может вести себя некорректно
    ratatui::restore();

    // Если во время работы произошел сбой, сообщаем об этом
    if let Err(err) = result {
        eprintln!("Критическая ошибка TUI: {:?}", err);
    }
    
    Ok(())
}
