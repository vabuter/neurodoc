# Устройство репозитория

Всё лежит в одном репозитории: три приложения, документация в двух видах, эталонный
каталог и скрипты сборки в корне. Здесь — что где искать и какие правила из этого следуют.

## Корень

```text
neurodoc/
├── windows/             приложение для Windows 11: C# / .NET 10, WinUI 3
├── linux/               приложение для Linux: Rust, GTK4 / libadwaita, WebKitGTK
├── android/             приложение для Android: Kotlin, Jetpack Compose
├── doc/                 документация проекта в формате neurodoc
├── example/             эталонный каталог neurodoc — выдуманный проект Fikus
├── book/                эта книга: book.toml и исходники в src/
├── .github/workflows/   pages.yml — сборка книги и выкладка на GitHub Pages
├── *.bat, *.sh          скрипты сборки, установки, выпуска версии и снимков для книги
├── README.md            обзор проекта и формата
├── CLAUDE.md            полный свод решений и подводных камней
├── LICENSE              лицензия MIT
├── .gitattributes       переводы строк
└── .gitignore
```

У каждого приложения свой README (`windows/README.md`, `linux/README.md`,
`android/README.md`): технологии, сборка, где лежат настройки. `CLAUDE.md` написан как
инструкция для ИИ-агента, но это и самое подробное описание устройства проекта.

## Двойники в трёх приложениях

Код разложен по-разному, но почти у каждого файла есть двойник на других платформах.
Пути в таблице — от `windows/NeurodocEditor/`, `linux/src/` и
`android/app/src/main/java/today/vab/neurodoceditor/`.

| Что | Windows | Linux | Android |
| --- | --- | --- | --- |
| Ядро каталога | `Services/NeurodocCatalog.cs` | `domain/catalog.rs` | `data/NeurodocCatalog.kt` |
| Git | `Services/GitService.cs` | `domain/git.rs` | `data/GitService.kt` |
| Миграция ссылок | `Services/CatalogMigration.cs` | `domain/migration.rs` | `data/CatalogMigration.kt` |
| Настройки | `Services/SettingsService.cs`, `Models/AppSettings.cs` | `domain/settings.rs` | `data/Settings.kt` |
| Прогресс чтения | `Services/ReadingProgressService.cs` | `domain/reading.rs` | `data/ReadingProgress.kt` |
| Окно, панели | `MainWindow.xaml(.cs)` | `ui/window/mod.rs`, `panels.rs` | `ui/EditorScreen.kt`, `ui/EditorViewModel.kt` |
| Редактор | `Controls/ChipView.cs`, `HandCanvas.cs` и оверлей в `MainWindow.xaml.cs` | `ui/editor/` | `ui/EditorText.kt`, поле — в `ui/EditorScreen.kt` |
| Поиск и замена | `MainWindow.Find.cs` | `ui/window/find.rs` | `ui/EditorViewModel.kt`, `ui/Dialogs.kt` |
| Просмотр форматов | `MainWindow.Viewer.cs` | `viewer/`, `ui/window/viewer.rs` | `ui/FormatViewer.kt` |
| Git в интерфейсе | `MainWindow.Git.cs` | `ui/window/git.rs` | `ui/GitDialogs.kt` |
| Диалоги | `Dialogs/` | `ui/window/dialogs.rs` | `ui/Dialogs.kt` |

Окно Windows-версии разбито на четыре partial-файла: навигация с редактором и панелями,
поиск, просмотр, git. `linux/src/ui/window/` разбито по тому же образцу, только мельче:
отдельно панели, ссылки, диалоги, сочетания клавиш (`shortcuts.rs`) и всплывающие
подсказки (`tip.rs`). Код Linux-версии собран библиотекой (`src/lib.rs`), а `main.rs`
только запускает интерфейс: так домен и просмотр доступны тестам `cargo test`.

## Общие ресурсы

Библиотеки просмотра (highlight.js, marked, Asciidoctor.js) и шрифты Inter и
JetBrains Mono лежат в `windows/NeurodocEditor/Assets`. Linux-версия вкладывает их в
исполняемый файл прямо оттуда (`include_str!` и `include_bytes!` в `linux/src/viewer/html.rs`);
оттуда же берутся значок в заголовке окна и иконки, которые ставит `linux-deploy.sh`.
Поэтому папку `linux/` нельзя собрать отдельно от остального репозитория.

У Android-версии своя копия тех же файлов: `android/app/src/main/assets/highlight` и
шрифты в `android/app/src/main/res/font`. Сейчас они побайтно совпадают с
Windows-версией. Обновляя библиотеку, меняйте обе копии.

## Одна функциональность — три версии

Пользовательские истории у всех трёх приложений одинаковые, поэтому правка формата или
поведения не считается законченной, пока её нет на всех платформах. Отличаться могут
только средства: где на Windows рисуется оверлей поверх текста, на Linux хватает тегов
текстового буфера, а на Android — преобразования показываемого текста (подробнее — в главе
[Архитектура](architecture.md)). Отличия в интерфейсе всё же есть. Номер версии на
Windows и Linux виден в заголовке окна, а на Android — внизу диалога настроек: так
задумано, верхняя панель телефона занята именем проекта. Своих сочетаний клавиш у
Android-версии нет — её команды собраны в меню, а чисто клавиатурных операций
(перемещение строк, отступ по <kbd>Tab</kbd>) там нет вовсе.

Те же правила касаются скриптов: у bat-файлов, которые имеют смысл на Linux, есть
sh-двойники (`android-build.sh`, `android-install.sh`, `release.sh`). Они повторяют
bat-файлы по проверкам и сообщениям, и правку одного доводят до другого.

## Документация: что где поправить

| Что изменилось | Где описать |
| --- | --- |
| Поведение или интерфейс | `doc/`, главы раздела [Neurodoc Editor](../editor/index.md), README версии |
| Формат | `README.md`, `doc/`, раздел [Формат neurodoc](../format/index.md), шаблон `ReadMe.md` в коде всех трёх версий |
| Сборка, скрипты | README, `doc/`, глава [Сборка](build.md) |
| Неочевидное решение, найденная грабля | `CLAUDE.md` и `doc/` |

Шаблон `ReadMe.md`, который приложение кладёт в новый каталог, — это константы
`ReadMeTemplate` (C#), `README_TEMPLATE` (Rust) и `READ_ME_TEMPLATE` (Kotlin).

`doc/` и книга рассказывают об одном, но по-разному: `doc/` открывают в самом редакторе
как проект, а книга — сайт для тех, у кого редактора ещё нет.

## Переводы строк и что не попадает в git

`.gitattributes` задаёт LF для всех текстовых файлов, кроме `*.bat` (у них CRLF — родной
формат cmd); `*.ttf`, `*.png`, `*.webp`, `*.ico`, `*.jar` и `*.apk` помечены двоичными и не
нормализуются. Без этого файла Git for Windows развернул бы при клоне всё в CRLF — и
`example/`, и `gradlew`, который с CRLF не запускается на Linux. Для каталогов neurodoc это
не косметика: редактор сохраняет файл в стиле его переводов строк, и каталог, однажды
развёрнутый в CRLF, таким бы и остался (см. [Каталог](../format/catalog.md#кодировка-и-переводы-строк)).

`.gitignore` отсекает выходные папки сборок (`bin/`, `obj/`, `linux/target/`, `build/`
Gradle, `book/build/`), `local.properties` с путём к Android SDK, ключи подписи (`*.jks`,
`*.keystore`) и файлы IDE.

## Эталонный каталог

`example/` — документация выдуманного сервиса Fikus (о нём — глава
[Каталог Fikus](../example.md)). На нём проверяют изменения: тесты Linux-версии копируют
его во временную папку, и ручные проверки идут так же — на копии. Сам `example/` при
тестировании не меняют.
