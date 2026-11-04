# Каталог глазами программы

Каталог neurodoc читают не только люди: сборочный скрипт или ИИ-агент разбирают его так
же, как редактор. Программе хватает таблицы «имя файла → текст» и одного регулярного
выражения. Ниже — алгоритм, общий для всех трёх версий Neurodoc Editor, и его перевод на
Python.

> [!TIP]
> Агенту, который видит каталог впервые, полезно заглянуть в `ReadMe.md` в корне: в
> каталоге, созданном Neurodoc Editor, там лежит краткое описание формата. Дальше — `root`
> и ссылки из него.

## Алгоритм

1. Прочитать `root` и файлы `_content` с именами-UUID; их id — `root` и UUID в нижнем
   регистре.
2. Обойти остальное дерево, не заходя в папки `_content` и папки с точкой в начале имени.
   Пропустить файлы с точкой в начале имени, больше 4 МБ и двоичные (с нулевым символом).
   id такого файла — путь от корня каталога через `/`.
3. Сложить всё в хеш-таблицу: ключи сравниваются без учёта регистра, но хранится
   написание с диска.
4. Искать ссылки регулярным выражением `\[([^\]\n]*)\]\(([^()\s\\]+)\)` — только в `root`
   и заметках `_content`.
5. Разобрать каждую цель относительно файла-источника. UUID — сразу id. Веб-адрес,
   абсолютный путь, двоеточие — отказ. Иначе это путь от папки источника (у `root` —
   корень, у заметки — `_content`): `.` пропускается, `..` поднимает на уровень вверх,
   подъём выше корня — отказ.
6. Привести результат к каноническому виду: `_content/<uuid>` → `<uuid>`, `Root` → `root`.
7. Цель существует, если в её id нет `..` и он есть в таблице или файл есть на диске —
   так редактор обходится с файлами, пропущенными на шаге 2.

## Пример на Python

```python
import os, re
from pathlib import Path

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", re.I)
LINK = re.compile(r"\[([^\]\n]*)\]\(([^()\s\\]+)\)")
MAX = 4 * 1024 * 1024  # крупные файлы пропускаем
def is_uuid(s): return UUID.fullmatch(s) is not None
def is_neurodoc(i): return i.lower() == "root" or is_uuid(i)
def read(p): return p.read_text(encoding="utf-8-sig", errors="replace")

def load(root):  # id в нижнем регистре → (id как на диске, текст)
    root = Path(root)
    files = {"root": ("root", read(root / "root"))}
    for f in (root / "_content").glob("*"):
        if f.is_file() and is_uuid(f.name):
            files[f.name.lower()] = (f.name.lower(), read(f))
    for top, dirs, names in os.walk(root):
        dirs[:] = [d for d in dirs if not d.startswith(".") and d.lower() != "_content"]
        for name in names:
            path = Path(top, name)
            rel = path.relative_to(root).as_posix()
            if name.startswith(".") or rel.lower() == "root" or path.stat().st_size > MAX:
                continue
            text = read(path)
            if "\0" not in text:  # двоичные файлы пропускаем
                files[rel.lower()] = (rel, text)
    return files

def resolve(source, target):  # цель ссылки → id файла или None
    target = target.strip()
    if is_uuid(target):
        return target.lower()  # UUID не зависит от того, где написана ссылка
    if not target or ":" in target or target[0] in "/\\":
        return None  # веб-адрес, абсолютный путь или диск
    parts = ([] if source.lower() == "root" else
             ["_content"] if is_uuid(source) else source.split("/")[:-1])
    for seg in target.replace("\\", "/").split("/"):
        if seg == "..":
            if not parts:
                return None  # подъём выше корня каталога
            parts.pop()
        elif seg not in ("", "."):
            parts.append(seg)
    if len(parts) == 2 and parts[0].lower() == "_content" and is_uuid(parts[1]):
        return parts[1].lower()  # _content/<uuid> — тот же файл, что <uuid>
    if len(parts) == 1 and parts[0].lower() == "root":
        return "root"
    return "/".join(parts) or None

def links(files, source):  # [(якорь, цель как написана, id цели или None)]
    sid, text = files[source.lower()]
    found = LINK.finditer(text) if is_neurodoc(sid) else []  # источники — root и _content
    return [(m[1], m[2], resolve(sid, m[2])) for m in found]

def title(files, i):  # первая непустая строка без «#»; у своих форматов — имя файла
    sid, text = files.get(i.lower(), (i, ""))
    if not is_neurodoc(sid):
        return sid.rsplit("/", 1)[-1]
    lines = (s.strip().lstrip("#").strip() for s in text.split("\n"))
    return next((s for s in lines if s), sid)
```

Битые ссылки во всём каталоге (без проверки диска из шага 7):

```python
files = load("fikus")
for sid, _ in list(files.values()):
    for anchor, raw, target in links(files, sid):
        web = raw.lower().startswith(("http://", "https://"))
        if not web and (not target or ".." in target or target.lower() not in files):
            print(f"{title(files, sid)}: [{anchor}]({raw})")
```

Обратные ссылки, сироты и порядок чтения строятся поверх `links()` — как именно,
рассказано в главе [Граф знаний](graph.md).

## Если программа пишет в каталог

- Кодировка — UTF-8 без BOM, переводы строк — как в файле (в каталогах обычно LF).
- Новая заметка — файл `_content/<uuid>` со случайным UUID (`str(uuid.uuid4())`), первая
  строка — заголовок. Чтобы заметка не осталась сиротой, сошлитесь на неё из существующей.
- Якорь — одна строка без `]`; в цели нет пробелов и скобок, путь считается от файла, в
  который пишется ссылка. Форма `<uuid>` — прежний формат, ссылкой она не считается.
- Удаляя файл, ссылки на него либо оставляют битыми, либо снимают: `[текст](цель)`
  превращается в `текст`, а ссылка с пустым якорем исчезает целиком. Так поступает
  редактор, когда при удалении выбран вариант «Файл и ссылки».
- Пример ссылки, который не должен работать, держат в файле собственного формата: там
  ссылки не разбираются.

Открытый редактор закрывать не нужно: Windows- и Linux-версии сами подхватывают правки на
диске и перезагружают открытый файл, если в нём нет несохранённых правок. На Android
каталог перечитывает пункт меню «Обновить с диска».
