# Neurodoc Editor для Linux

Десктопная версия редактора каталогов **neurodoc** с теми же функциями и оформлением,
что и Windows-версия из `windows/`: тёмная графитовая тема, teal-акцент, компактные
ссылки, навигация как в браузере, прогресс чтения, автосохранение, просмотр форматов,
git. Описание формата — в [README.md](../README.md) в корне репозитория.

## Технологии

- Rust (edition 2024), [gtk4-rs](https://gtk-rs.org/): GTK4 + libadwaita, WebKitGTK 6.0
  для просмотра форматов и дельт
- Нижняя планка нативной сборки — GTK 4.14 / libadwaita 1.5 (Ubuntu 24.04, Debian 13,
  Fedora 40 и новее). Нативно на Wayland с точным дробным масштабированием; на X11 (Cinnamon,
  XFCE) тоже работает, масштаб там только целый
- Настройки — JSON в `~/.config/neurodoc-editor/` (формат тот же, что у Android-версии)
- Библиотеки просмотра (highlight.js, marked.js, Asciidoctor.js) и шрифты Inter и
  JetBrains Mono вкладываются в бинарник из `windows/NeurodocEditor/Assets` — всё работает
  офлайн

## Сборка и запуск

Пакеты (Fedora):

```bash
sudo dnf install rust cargo rustfmt clippy gtk4-devel libadwaita-devel webkitgtk6.0-devel glib2-devel
```

Debian/Ubuntu: `sudo apt install libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev`, а
компилятор — через [rustup](https://rustup.rs): нужен rustc 1.92 или новее, в репозиториях
Ubuntu 24.04 и Debian 13 он старше. Планка GTK 4.14 / libadwaita 1.5 относится только к
системным библиотекам.

Из корня репозитория:

```bash
./linux-build.sh
```

```bash
./linux-run.sh
```

Установка для текущего пользователя (без root): бинарник в `~/.local/opt/neurodoc-editor` и
ссылка на него в `~/.local/bin`, ярлык
`today.vab.NeurodocEditor.desktop` и иконки в `~/.local/share` — приложение появляется в
сетке приложений GNOME:

```bash
./linux-deploy.sh
```

Вручную (из `linux/`): `cargo build --release`, исполняемый файл —
`target/release/neurodoc-editor`. Версия берётся из git-тега скриптом `build.rs`, в
`Cargo.toml` её нет намеренно (см. «Версионирование» в корневом README).

## Проверка

```bash
cd linux && cargo test
```

Тесты домена идут на копии `example/` во временной папке, включая весь рабочий цикл git на
локальном bare-репозитории (без сети и ключей). Сам `example/` при этом не меняется.

Самопроверка интерфейса: приложение само проходит по файлам, режимам просмотра и диалогам
и кладёт снимок окна после каждого шага.

```bash
XDG_CONFIG_HOME=/tmp/nd/config NEURODOC_SMOKE=/tmp/nd/shots ./linux/target/debug/neurodoc-editor
```

То, чего не видно на снимке, она проверяет сама и пишет в stderr строками `smoke: попадание …`
(наведение ловит каждая ссылка строки, а не только первая), `smoke: прокрутка …`
(не сбрасывается ли прокрутка редактора и просмотра MD), `smoke: курсор …` («рука» над
ссылкой) и `smoke: слежение …` (своё автосохранение не вызывает перечитывания, а чужая правка
сразу после него подхватывается; шаг сам правит `root` и `README.md` копии и возвращает их).
Для проверки прокрутки эти файлы должны быть длиннее окна, иначе в журнале будет «не
проверялась».

Переменная `XDG_CONFIG_HOME` даёт изолированный экземпляр с собственными настройками —
так проверяют приложение на копии `example/`, не трогая настоящие проекты. Одиночный
снимок окна — `NEURODOC_SNAPSHOT=/путь.png`.

## Где лежат файлы

| Что | Где |
| --- | --- |
| настройки, проекты | `~/.config/neurodoc-editor/settings.json` |
| прогресс чтения | `~/.config/neurodoc-editor/reading/<проект>.json` |
| резервные копии миграции | `~/.local/share/neurodoc-editor/backups/` |
| журнал ошибок | `~/.local/state/neurodoc-editor/error.log` |
| встроенные шрифты | `~/.local/share/fonts/neurodoc-editor/` |

## Git

Используется системный `git` и его настройки. Запрос пароля в терминале отключён
(`GIT_TERMINAL_PROMPT=0`), поэтому для HTTPS нужен credential helper (`git config --global
credential.helper libsecret`), а проще всего работать по SSH-ключам через агент GNOME.

## Структура

```
linux/
├── Cargo.toml, build.rs         — версия из git describe
├── data/*.desktop.in            — шаблон ярлыка
└── src/
    ├── domain/                  — порт домена: каталог, ссылки, git, настройки, чтение, миграция
    ├── viewer/                  — страницы просмотра (html.rs) и скрипты, общие с Windows и Android
    └── ui/
        ├── editor/              — GtkTextView с компактными ссылками на тегах
        ├── window/              — окно: панели, ссылки, поиск, просмотр, git, диалоги, клавиши
        ├── css.rs, fonts.rs     — оформление и встроенные шрифты
        └── smoke.rs             — самопроверка интерфейса
```

Как устроены компактные ссылки на тегах и какие правила из этого следуют — в `CLAUDE.md`
(раздел «Linux-версия») и в документации `doc/`.
