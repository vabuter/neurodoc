# Установка

Готовых сборок Neurodoc Editor пока нет: установить его — значит собрать из исходников. Для
каждой платформы в корне репозитория лежат скрипты, которые делают всё сами; от вас нужны
только инструменты сборки.

## Исходники

```bash
git clone https://github.com/vabuter/neurodoc.git
cd neurodoc
```

Собирайте из клона, а не из скачанного архива: номер версии все три сборки берут из git-тега
`vX.Y.Z`. Без git приложение соберётся, но покажет версию `0.0.0-nogit` — подробнее в главе
[Версии и выпуски](../dev/versioning.md).

## Linux

Нужны компилятор Rust 1.92 или новее и пакеты разработки GTK4, libadwaita и WebKitGTK 6.0.
Нижняя планка системных библиотек — GTK 4.14, libadwaita 1.5 и WebKitGTK 2.42: Ubuntu 24.04,
Debian 13, Fedora 40 и новее. Приложение работает нативно на Wayland с дробным масштабированием; на X11
тоже работает, но масштаб там только целый.

Fedora:

```bash
sudo dnf install rust cargo rustfmt clippy gtk4-devel libadwaita-devel webkitgtk6.0-devel glib2-devel
```

Debian и Ubuntu — библиотеки из репозитория, а компилятор через [rustup](https://rustup.rs):
rustc в Ubuntu 24.04 и Debian 13 старше нужного.

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
```

В других дистрибутивах ищите пакеты разработки, которые `pkg-config` находит под именами
`gtk4`, `libadwaita-1` и `webkitgtk-6.0`: именно так их наличие проверяет скрипт сборки.

Скрипты запускают из корня репозитория:

| Скрипт | Что делает |
| --- | --- |
| `./linux-build.sh` | release-сборка, результат — `linux/target/release/neurodoc-editor`; с ключом `--debug` — отладочная сборка в `linux/target/debug/` |
| `./linux-run.sh` | собирает и сразу запускает; ключ `--debug` тоже понимает |
| `./linux-deploy.sh` | release-сборка и установка для текущего пользователя, без прав root |

[`linux-deploy.sh`](https://github.com/vabuter/neurodoc/blob/main/linux-deploy.sh) раскладывает
файлы по папкам пользователя:

- программа — `~/.local/opt/neurodoc-editor/neurodoc-editor`;
- ссылка на неё — `~/.local/bin/neurodoc-editor`, чтобы запускать из терминала;
- ярлык — `~/.local/share/applications/today.vab.NeurodocEditor.desktop`: приложение
  появляется в сетке приложений GNOME;
- иконки — `~/.local/share/icons/hicolor/<N>x<N>/apps/`.

Пока приложение запущено, скрипт работать откажется: закройте окно и повторите. В конце он
печатает команду для удаления. Без скриптов приложение собирается командой
`cargo build --release` в папке `linux/`.

При запуске приложение копирует встроенные шрифты Inter и JetBrains Mono в
`~/.local/share/fonts/neurodoc-editor` — их видят и редактор, и просмотр форматов. Настройки и
список проектов лежат в `~/.config/neurodoc-editor/settings.json`.

> [!TIP]
> Чтобы попробовать приложение, не трогая список своих проектов, запустите отдельный экземпляр
> со своими настройками: `XDG_CONFIG_HOME=/tmp/nd/config ./linux-run.sh`. Окон может быть
> открыто несколько одновременно.

## Windows

Windows-версию собирают только на Windows: компилятор XAML и Windows App SDK работают лишь
там. Нужны [.NET 10 SDK](https://dotnet.microsoft.com/download) и git.

| Скрипт | Что делает |
| --- | --- |
| `build-debug.bat` | Debug-сборка (x64) |
| `build-release.bat` | Release-сборка (x64) |
| `build-deploy.bat` | Release-сборка и публикация в папку деплоя |

[`build-deploy.bat`](https://github.com/vabuter/neurodoc/blob/main/build-deploy.bat) публикует
приложение в `C:\d\deployments\neurodoc_editor`; папку меняют переменной `DEPLOY_DIR` в начале
скрипта. Публикация самодостаточная: в папке лежат и рантайм .NET, и Windows App SDK, так что
программа запускается на любой Windows 11 x64 без установки зависимостей. Установщика нет —
запускают файл `NeurodocEditor.exe` из этой папки. Пока приложение открыто,
скрипт откажется работать: иначе в папке смешались бы новые файлы со старым exe.

Как собрать без скриптов, в том числе под ARM64, рассказано в главе
[Сборка](../dev/build.md#windows).

Просмотр форматов работает на WebView2, который входит в Windows 11. При первом запуске
приложение ставит шрифты Inter и JetBrains Mono для текущего пользователя, без прав
администратора; если после самого первого запуска встроенный шрифт не применился к тексту,
перезапустите приложение. Настройки лежат в `%APPDATA%\NeurodocEditor\settings.yaml`.

Bat-скрипты при ошибке ждут нажатия клавиши.

## Android

Нужны Android SDK и JDK — проще всего поставить [Android Studio](https://developer.android.com/studio).
Демон Gradle работает на JDK 21 и при необходимости скачивает его сам. Приложение рассчитано на
Android 11 и новее.

| Скрипт | Что делает |
| --- | --- |
| `android-build.bat`, `android-build.sh` | debug-сборка: `android/app/build/outputs/apk/debug/app-debug.apk` |
| `android-install.bat`, `android-install.sh` | сборка, установка на телефон, подключённый по USB, выдача доступа к файлам и запуск |

`.bat` — для Windows, `.sh` — для Linux; bat-файлы при ошибке ждут нажатия клавиши, sh-скрипты
нет. Вручную — `./gradlew assembleDebug` из папки `android/` (на Windows —
`gradlew.bat assembleDebug`).

Где сборка ищет SDK:

1. `android/local.properties` — его пишет Android Studio, в git он не попадает;
2. переменные окружения `ANDROID_HOME` и `ANDROID_SDK_ROOT`;
3. если не задано ничего, sh-скрипты подставляют `~/.local/opt/android-sdk` — этот же путь
   можно указать Android Studio при установке SDK.

`android-install.bat` берёт adb из `%LOCALAPPDATA%\Android\Sdk\platform-tools`, а
`android-install.sh` — из найденного SDK, иначе из `PATH`. Если к компьютеру подключено
несколько телефонов, нужный выбирают переменной `ANDROID_SERIAL`.

> [!WARNING]
> Debug-APK подписан ключом той машины, где собран (`~/.android/debug.keystore`). Поверх
> приложения, собранного на другой машине, он не встанет — установка закончится ошибкой
> `INSTALL_FAILED_UPDATE_INCOMPATIBLE`. Либо перенесите ключ с той машины, либо удалите
> приложение (`adb uninstall today.vab.neurodoceditor`), а вместе с ним — его настройки,
> список проектов и git-токены.

Каталоги neurodoc лежат в общей памяти телефона, поэтому приложению нужен доступ ко всем
файлам. `android-install` выдаёт его сам; иначе приложение попросит доступ при запуске.
Выдать его можно и вручную:

```bash
adb shell appops set --uid today.vab.neurodoceditor MANAGE_EXTERNAL_STORAGE allow
```

Подробности для тех, кто будет править код, — в главе [Сборка](../dev/build.md).
