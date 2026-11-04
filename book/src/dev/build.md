# Сборка

Готовых сборок пока нет: каждое приложение собирается из исходников скриптами из корня
репозитория. Какие инструменты поставить и что делает каждый скрипт, рассказано в главе
[Установка](../editor/install.md), — здесь это не повторяется. Эта глава — для тех, кто
правит код: сборка без скриптов, подводные камни платформ, скрипты в автоматизации и сборка
этой книги.

| Что собираем | Где | Скрипты в корне |
| --- | --- | --- |
| Windows-версию | только на Windows | `build-debug.bat`, `build-release.bat`, `build-deploy.bat` |
| Linux-версию | на Linux | `linux-build.sh`, `linux-run.sh`, `linux-deploy.sh` |
| Android-версию | на Windows и на Linux | `android-build.bat` / `.sh`, `android-install.bat` / `.sh` |
| Эту книгу | где есть mdBook | `mdbook build book` |

Все три приложения берут номер версии из git-тега через `git describe`, поэтому
собирайте их из клона репозитория: в архиве без `.git` версия получится `0.0.0-nogit`.
Тег ставит `./release.sh X.Y.Z` (на Windows — `release.bat X.Y.Z`); подробнее — в главе
[Версии и выпуски](versioning.md).

## Windows

Пакеты Windows App SDK приходят из NuGet при первой сборке.

- `build-debug.bat` и `build-release.bat` собирают x64 и печатают путь к exe:
  `windows\NeurodocEditor\bin\x64\Debug\net10.0-windows10.0.22621.0\win-x64\NeurodocEditor.exe`
  (у Release — `Release` вместо `Debug`).
- `build-deploy.bat` удаляет прежнюю папку деплоя, только если она похожа на прошлую
  выкладку — в ней лежит `NeurodocEditor.exe`, — а после публикации проверяет, что exe на
  месте. Что попадает в эту папку, рассказано в главе
  [Установка](../editor/install.md#windows).

Вручную, из папки `windows/`:

```bash
dotnet build NeurodocEditor/NeurodocEditor.csproj -c Debug -p:Platform=x64
```

> [!WARNING]
> Платформу указывайте явно: `-p:Platform=x64` или `-p:Platform=ARM64`, как это делают
> скрипты. Если сборка получит AnyCPU, self-contained сборка Windows App SDK упадёт с
> ошибкой «requires a supported Windows architecture». Страховки есть — платформы явно
> сопоставлены в `NeurodocEditor.slnx`, а `NeurodocEditor.csproj` подменяет пустую
> платформу и AnyCPU на x64, — но полагаться на умолчания не стоит.

Папку `Assets` (шрифты, библиотеки просмотра, иконки) в выходную папку копируют
собственные таргеты `CopyAppAssets` и `CopyAppAssetsOnPublish` из `.csproj`: стандартные
правила Windows App SDK этого не делают.

На Linux Windows-версию не собрать и не запустить: компилятор XAML и Windows App SDK
работают только под Windows. Код на C# там можно править, а собирать и проверять — на
Windows.

## Linux

Зависимости и команды установки пакетов — в главе [Установка](../editor/install.md#linux).
Нижнюю планку держит `linux/Cargo.toml`: `rust-version = "1.92"` и фичи крейтов `v4_14`,
`v1_5` и `v2_42` — с ними не скомпилируется вызов API новее GTK 4.14, libadwaita 1.5 и
WebKitGTK 2.42.

```bash
./linux-build.sh --debug   # debug: linux/target/debug/neurodoc-editor
./linux-run.sh --debug     # debug-сборка и запуск
```

- `linux-build.sh` сначала проверяет `cargo` и пакеты разработки `gtk4`, `libadwaita-1`,
  `webkitgtk-6.0` (через `pkg-config`) и, если чего-то нет, печатает команду установки.
- `linux-run.sh` принимает тот же `--debug`, а остальные аргументы передаёт приложению.
- `linux-deploy.sh` ставит ярлык `today.vab.NeurodocEditor.desktop`. Имя ярлыка обязано
  совпадать с id приложения: по нему GNOME на Wayland находит иконку окна.

Вручную: `cd linux && cargo build --release`. Сборке нужна папка `windows/`: библиотеки
просмотра, шрифты и значок заголовка окна Linux-версия берёт из
`windows/NeurodocEditor/Assets` (см. [Устройство репозитория](repository.md#общие-ресурсы)).

## Android

Что поставить и где сборка ищет Android SDK — в главе
[Установка](../editor/install.md#android). Gradle ставить не нужно: его скачивает обёртка
`gradlew`, она лежит в репозитории. Вручную, из папки `android/`: `./gradlew assembleDebug`
(на Windows — `gradlew.bat assembleDebug`).

- **Установка** — `adb install -r`, затем выдача доступа «Ко всем файлам»
  (`MANAGE_EXTERNAL_STORAGE`) через `appops` и запуск приложения. Если подключено
  несколько устройств, нужное выбирают переменной `ANDROID_SERIAL` — её понимает сам adb.
- **Чужой ключ подписи.** Debug-APK не встанет поверх приложения, собранного на другой
  машине (`INSTALL_FAILED_UPDATE_INCOMPATIBLE`, подробности — в главе
  [Установка](../editor/install.md#android)). `android-install.sh` эту ошибку распознаёт и
  подсказывает выход.

## Скрипты и автоматизация

- `build-*.bat` и `android-*.bat` при ошибке выполняют `pause` и ждут нажатия клавиши: из
  автоматизации процесс провисит до таймаута. Там зовите `dotnet build … -p:Platform=x64` и
  `android\gradlew.bat assembleDebug` напрямую.
- sh-скрипты клавиши не ждут, их можно звать как есть. У `build-*.bat` sh-двойников нет:
  на Linux Windows-версия не собирается.
- bat-файлы написаны по-английски и в ASCII: cmd ломает разбор UTF-8 с кириллицей даже
  после `chcp 65001`.

## Книга

Сайт, который вы читаете, собирает [mdBook](https://rust-lang.github.io/mdBook/) 0.5.4:
настройки — в `book/book.toml`, страницы — в `book/src/`, оглавление — в
`book/src/SUMMARY.md`, снимки экрана Linux-версии — в `book/src/images/`. Поставить
mdBook можно готовым файлом со [страницы выпусков](https://github.com/rust-lang/mdBook/releases)
или через cargo:

```bash
cargo install mdbook --version 0.5.4 --locked
```

Из корня репозитория:

```bash
mdbook serve book --open   # http://localhost:3000, пересборка при каждой правке
mdbook build book          # готовый сайт в book/build/
```

Снимки экрана пересоздаёт одна команда из корня репозитория:

```bash
./book-shots.sh
```

Скрипт собирает Linux-версию и делает копию `example/` git-репозиторием с тремя
выдуманными коммитами (для карточки истории и дельты). Затем запускает на копии
[самопроверку](testing.md#самопроверка-интерфейса) в собственном безэкранном композиторе:
`mutter --headless` в отдельной сессии D-Bus. Поэтому окно не мешает работе, а
заблокированный экран не портит снимки (у заблокированной сессии они выходят пустыми).
Нужные шаги скрипт перекладывает в `book/src/images/` в WebP без потерь: так вчетверо
меньше PNG при тех же пикселях. Нужны mutter, ImageMagick 7 (`magick`) и git. Номер
версии в заголовке окна берётся из тега, поэтому снимать лучше на чистом дереве, иначе
там будет «-dirty».

В `book.toml` стоит `create-missing = false`: страница, которая есть в `SUMMARY.md`, но не
создана, — ошибка сборки («failed to read chapter»), а не пустая заглушка. Папка
`book/build/` в репозиторий не попадает.

Выкладывает книгу GitHub Actions — workflow `.github/workflows/pages.yml`. Он запускается
при пуше в `main`, если менялись `book/` или сам workflow, а также вручную. Workflow
скачивает mdBook той же версии, выполняет `mdbook build book` и выкладывает
`book/build` на GitHub Pages: <https://vabuter.github.io/neurodoc/>. Выкладки идут по одной,
начатая не прерывается. В новом репозитории, в том числе в форке, выкладку нужно один раз
включить в настройках: Settings → Pages → Source: GitHub Actions.
