package today.vab.neurodoceditor.data

import java.io.File

data class SearchHit(
    val fileId: String,
    val title: String,
    val prefix: String,
    val match: String,
    val suffix: String,
    val occurrence: Int,
)

data class FileRef(val id: String, val title: String)

/** Ссылка на несуществующий файл: где найдена и куда ведёт. */
data class BrokenLink(
    val fileId: String,
    val fileTitle: String,
    val targetId: String,
    val position: Int,
    val length: Int,
    val context: String,
)

/**
 * Ссылка в тексте файла с уже разобранной целью.
 *
 * @param target цель как написана в тексте — её показывают в «Битых ссылках».
 * @param id файл каталога, на который она ведёт, или null: веб-адрес либо путь наружу каталога.
 */
data class LinkRef(
    val start: Int,
    val length: Int,
    val target: String,
    val anchor: String,
    val id: String?,
) {
    val end: Int get() = start + length
}

/**
 * Каталог документации neurodoc: корневой файл root + файлы-UUID в папке _content
 * + файлы в собственных форматах (YAML, SVG, конфиги) в доменных папках.
 * Держит содержимое всех файлов в памяти (id -> контент), как и Windows-версия.
 */
class NeurodocCatalog private constructor(val rootPath: String) {

    /**
     * Содержимое файлов каталога. Сравнение имён БЕЗ УЧЁТА РЕГИСТРА, как в Windows-версии:
     * файловые системы, где лежат каталоги, регистр обычно не различают, и «ReadMe.md»
     * должен находить «README.md» — иначе тот же файл под другим написанием выглядел бы
     * как чужой, без заголовка и обратных ссылок. Сам ключ хранится как на диске:
     * у пути регистр — часть имени.
     */
    private val files: MutableMap<String, String> =
        java.util.TreeMap(String.CASE_INSENSITIVE_ORDER)

    /**
     * Разобранные ссылки файлов. Панели (пути, обратные ссылки, сироты, битые, порядок
     * чтения) перестраиваются после каждого автосохранения, и без кэша каждая заново
     * прогоняла регулярку по всему каталогу.
     *
     * Разбор зависит только от текста файла и его папки, поэтому запись сбрасывает
     * запись одного файла, а не весь кэш. Любая новая мутация [files] обязана
     * сбросить и [linkCache].
     */
    private val linkCache: MutableMap<String, List<LinkRef>> =
        java.util.TreeMap(String.CASE_INSENSITIVE_ORDER)

    val contentPath: String get() = File(rootPath, CONTENT_DIR).path

    companion object {
        const val ROOT_ID = "root"

        /**
         * Папка с neurodoc-файлами. Подчёркивание в начале помечает папку служебной, но,
         * в отличие от точки, не делает её скрытой: каталог смотрят и обычным файловым
         * менеджером, и с телефона. Обычное «content» остаётся свободным под прикладную
         * папку самого проекта.
         */
        const val CONTENT_DIR = "_content"

        /** Больше этого файл в память не берём — каталог не хранилище двоичных данных. */
        private const val MAX_FILE_BYTES = 4L * 1024 * 1024

        /**
         * Ссылка формата neurodoc: `[текст](цель)`. Группа 1 — текст ссылки (якорь,
         * может быть пустым), группа 2 — цель: UUID файла из _content/ либо путь
         * к файлу каталога ОТНОСИТЕЛЬНО того файла, где написана ссылка.
         * Якорь однострочный и не содержит "]".
         *
         * Сама по себе цель неполна — её разбирает [tryResolveLink].
         */
        val LINK_REGEX = Regex("""\[([^\]\n]*)\]\(([^()\s\\]+)\)""")

        /**
         * Ссылка старого формата `<uuid>`. Ссылкой больше не считается и нужна только
         * миграции каталога на новый формат (см. CatalogMigration).
         */
        val LEGACY_LINK_REGEX =
            Regex("<([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})>")

        /** HTTP/HTTPS-ссылка в тексте. */
        val WEB_LINK_REGEX = Regex("""https?://[^\s<>"]+""", RegexOption.IGNORE_CASE)

        private val ID_REGEX =
            Regex("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")

        /** UUID в произвольном месте строки (для разбора пользовательского ввода). */
        private val ANY_ID_REGEX =
            Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")

        fun openOrCreate(path: String): NeurodocCatalog {
            ensureStructure(path)
            return NeurodocCatalog(path).apply { reload() }
        }

        /** Создаёт каталог проекта, файл root, папку _content и ReadMe.md, если их ещё нет. */
        fun ensureStructure(path: String) {
            val dir = File(path)
            dir.mkdirs()
            File(dir, CONTENT_DIR).mkdirs()

            val rootFile = File(dir, ROOT_ID)
            if (!rootFile.exists()) rootFile.writeText("")

            val hasReadme = dir.listFiles()?.any { it.name.equals("readme.md", ignoreCase = true) } == true
            if (!hasReadme) File(dir, "ReadMe.md").writeText(READ_ME_TEMPLATE)
        }

        /** UUID ли это — то есть файл из папки _content. */
        fun isUuid(id: String): Boolean = ID_REGEX.matches(id)

        /**
         * Файл формата neurodoc (root или файл из _content) — в отличие от файлов
         * в собственных форматах, которые лежат в своих папках и читаются в том
         * числе программами.
         */
        fun isNeurodocFile(id: String): Boolean =
            id.equals(ROOT_ID, ignoreCase = true) || isUuid(id)

        /** Папка файла относительно корня каталога: "" для root, "_content" для UUID. */
        fun dirOf(id: String): String = when {
            id.equals(ROOT_ID, ignoreCase = true) -> ""
            isUuid(id) -> CONTENT_DIR
            else -> id.substringBeforeLast('/', "")
        }

        /** Ссылка на внешний ресурс, а не на файл каталога. */
        fun isWebTarget(target: String): Boolean =
            target.startsWith("http://", ignoreCase = true) ||
                target.startsWith("https://", ignoreCase = true)

        /**
         * Годен ли id как имя файла каталога: за его пределы формат ссылок не выпускает.
         */
        fun isValidId(id: String): Boolean =
            id.isNotBlank() && !id.contains("..") && !id.contains(':') &&
                id[0] != '/' && id[0] != '\\'

        /**
         * Разбирает цель ссылки в id файла. Путь считается ОТ ФАЙЛА, в котором написана
         * ссылка, поэтому нужен [sourceId].
         *
         * @return null, если цель — веб-адрес, абсолютный путь или ведёт за пределы
         * каталога: по формату ссылок наружу не бывает, и разыменовывать их нельзя.
         */
        fun tryResolveLink(sourceId: String, rawTarget: String): String? {
            if (rawTarget.isBlank()) return null
            val target = rawTarget.trim()
            if (isWebTarget(target)) return null
            if (isUuid(target)) return target.lowercase()
            // абсолютные пути и диски каталогу не принадлежат
            if (target.contains(':') || target[0] == '/' || target[0] == '\\') return null

            val parts = dirOf(sourceId).split('/').filter { it.isNotEmpty() }.toMutableList()
            for (segment in target.replace('\\', '/').split('/')) {
                if (segment.isEmpty() || segment == ".") continue
                if (segment == "..") {
                    // подъём выше корня каталога — ссылка наружу, такой в формате нет
                    if (parts.isEmpty()) return null
                    parts.removeAt(parts.lastIndex)
                    continue
                }
                parts.add(segment)
            }
            if (parts.isEmpty()) return null

            // приводим к каноническому виду: _content/<uuid> — это тот же id, что и <uuid>
            if (parts.size == 2 && parts[0].equals(CONTENT_DIR, ignoreCase = true) && isUuid(parts[1])) {
                return parts[1].lowercase()
            }
            if (parts.size == 1 && parts[0].equals(ROOT_ID, ignoreCase = true)) return ROOT_ID
            return parts.joinToString("/")
        }

        /**
         * Путь цели относительно папки, из которой на неё сошлются, — обратная операция
         * к разбору ссылки. Папку берут через [dirOf].
         */
        fun makeRelativeTarget(sourceDir: String, targetId: String): String {
            if (isUuid(targetId)) return targetId

            val from = sourceDir.split('/').filter { it.isNotEmpty() }
            val to = targetId.split('/').filter { it.isNotEmpty() }
            var common = 0
            while (common < from.size && common < to.size &&
                from[common].equals(to[common], ignoreCase = true)
            ) common++

            val parts = mutableListOf<String>()
            repeat(from.size - common) { parts.add("..") }
            for (i in common until to.size) parts.add(to[i])
            return if (parts.isEmpty()) targetId else parts.joinToString("/")
        }

        /**
         * Разбирает пользовательский ввод в id файла: "root", голый UUID,
         * ссылка в скобках или произвольный текст, внутри которого есть UUID.
         */
        fun parseId(input: String): String? {
            if (input.isBlank()) return null
            val trimmed = input.trim().trim('<', '>', '(', ')').trim()
            if (trimmed.equals(ROOT_ID, ignoreCase = true)) return ROOT_ID
            return ANY_ID_REGEX.find(input)?.value?.lowercase()
        }

        fun shorten(id: String): String = if (id.length > 8) id.take(8) + "…" else id

        val READ_ME_TEMPLATE = listOf(
            "# Каталог документации в формате neurodoc",
            "",
            "Формат называется neurodoc.",
            "Формат neurodoc представляет собой набор текстовых файлов.",
            "Внутри каждого файла находится блок знаний или информации.",
            "Все файлы в кодировке UTF-8.",
            "Начинать читать надо с корневого файла - он всегда называется root.",
            "Остальные файлы находятся в папке _content.",
            "Их имена являются уникальными идентификаторами в рамках данного каталога.",
            "Идентификаторы - это UUID в нижнем регистре.",
            "В тексте любого файла может встретиться ссылка вида [текст](uuid).",
            "Это значит, что это ссылка на другой файл в папке _content с таким именем,",
            "а текст в квадратных скобках - её якорь (текст ссылки), встроенный в предложение.",
            "Текст ссылки может быть пустым - тогда показывают заголовок целевого файла.",
            "Текст ссылки не содержит символа \"]\" и переводов строк.",
            "UUID сам по себе, в том числе обрамлённый < и >, ссылкой не считается.",
            "Кроме папки _content в каталоге могут быть папки с файлами в собственных форматах",
            "(YAML, SVG, конфиги). Имена у них не UUID, а доменные: эти файлы читают и программы.",
            "В ссылке вместо UUID может стоять путь к такому файлу - относительно того файла,",
            "в котором написана ссылка. Например, ссылка [auth.start](../contract/auth/start.yaml),",
            "написанная в файле из папки _content, ведёт на файл contract/auth/start.yaml.",
            "Ссылка по пути ведёт только внутрь каталога neurodoc. За пределы каталога ссылок нет.",
            "Таким образом, файлы образуют граф знаний с корнем в файле root.",
        ).joinToString("\n")
    }

    fun reload() {
        files.clear()
        linkCache.clear()
        files[ROOT_ID] = readFromDisk(ROOT_ID)
        File(contentPath).listFiles()?.forEach { file ->
            if (ID_REGEX.matches(file.name)) {
                runCatching { files[file.name.lowercase()] = file.readText() }
            }
        }

        // Кроме _content в каталоге бывают папки с файлами в собственных форматах
        // (YAML, SVG, конфиги) — на них тоже ссылаются, по пути от файла-источника.
        for (file in enumerateDomainFiles(File(rootPath))) {
            val rel = file.path.removePrefix(rootPath).trimStart('/', '\\').replace('\\', '/')
            if (rel.equals(ROOT_ID, ignoreCase = true)) continue
            runCatching {
                if (file.length() > MAX_FILE_BYTES) return@runCatching
                val text = file.readText()
                if (text.contains('\u0000')) return@runCatching // двоичный файл читать нечем
                files[rel] = text
            }
        }
    }

    /**
     * Файлы каталога в собственных форматах: всё дерево, кроме служебных папок
     * (_content, .git и прочих с точки) и файлов, имя которых начинается с точки.
     *
     * Обходим папки сами: обход «всё дерево, потом отфильтровать» сначала спускается
     * в .git и перечисляет всё, что там лежит. На настоящем репозитории это десятки
     * тысяч лишних имён при каждом перечитывании.
     */
    private fun enumerateDomainFiles(root: File): List<File> {
        val result = mutableListOf<File>()
        val pending = ArrayDeque<File>()
        pending.add(root)
        while (pending.isNotEmpty()) {
            val dir = pending.removeFirst()
            val children = dir.listFiles() ?: continue
            for (child in children) {
                if (child.name.startsWith('.')) continue
                if (child.isDirectory) {
                    // _content отбрасываем ПО ИМЕНИ: точка его больше не прячет, и без
                    // этой проверки файлы-UUID попали бы в каталог второй раз — как файлы
                    // в собственном формате с путём «_content/<uuid>»
                    if (child.name.equals(CONTENT_DIR, ignoreCase = true)) continue
                    pending.add(child)
                } else {
                    result.add(child)
                }
            }
        }
        return result
    }

    fun allIds(): List<String> = files.keys.sortedBy { if (it == ROOT_ID) "" else it }

    /**
     * Файлы, в которых имеет смысл искать ссылки: root и _content.
     *
     * Файлы в собственных форматах — цели ссылок, а не источники. Разбирать их как
     * neurodoc опасно: последовательность вида `[текст](что-то)` там встречается сама
     * по себе. На этом попался ReadMe.md — примеры из описания формата приложение
     * считало настоящими ссылками и жаловалось на битые.
     */
    private fun linkSourceIds(): List<String> = files.keys.filter { isNeurodocFile(it) }

    /**
     * Путь к файлу каталога.
     *
     * @throws IllegalArgumentException id ведёт за пределы каталога. Такие сюда попадать
     * не должны: их отсеивает [tryResolveLink]. Молчаливая подмена на root приводила бы
     * к тому, что [exists] отвечает «файл есть», а запись ушла бы в root.
     */
    fun filePath(id: String): String = when {
        id.equals(ROOT_ID, ignoreCase = true) -> File(rootPath, ROOT_ID).path
        isUuid(id) -> File(contentPath, id).path
        !isValidId(id) -> throw IllegalArgumentException("Путь ведёт за пределы каталога: $id")
        else -> File(rootPath, id).path
    }

    fun readFile(id: String): String = files[id] ?: readFromDisk(id)

    private fun readFromDisk(id: String): String =
        runCatching { File(filePath(id)).takeIf { it.exists() }?.readText() ?: "" }.getOrDefault("")

    fun writeFile(id: String, text: String) {
        val file = File(filePath(id))
        file.parentFile?.mkdirs()
        file.writeText(text)
        files[id] = text
        linkCache.remove(id)
    }

    /** Есть ли такой файл в каталоге. Негодный id — всегда «нет». */
    fun exists(id: String): Boolean =
        isValidId(id) && (files.containsKey(id) || runCatching { File(filePath(id)).exists() }.getOrDefault(false))

    /**
     * Разбирает то, что ввёл пользователь, в id файла ЭТОГО каталога: «root», UUID
     * (в том числе внутри произвольного текста) или путь вида contract/auth/start.yaml.
     *
     * [parseId] про пути ничего не знает — он ищет UUID в отрыве от каталога, а в списке
     * файлов проекта такие пути есть.
     */
    fun resolveInput(input: String): String? {
        if (input.isBlank()) return null
        parseId(input)?.let { if (exists(it)) return it }
        val path = input.trim().replace('\\', '/')
        return if (isValidId(path) && exists(path)) path else null
    }

    fun createNewFile(): String {
        val id = java.util.UUID.randomUUID().toString().lowercase()
        writeFile(id, "")
        return id
    }

    fun deleteFile(id: String) {
        if (id == ROOT_ID) return
        runCatching { File(filePath(id)).delete() }
        files.remove(id)
        linkCache.remove(id)
    }

    /**
     * Снимает во всех файлах каталога ссылки на указанный: `[текст](цель)` превращается
     * в `текст`. Возвращает число изменённых файлов.
     *
     * Текст ссылки остаётся намеренно — его писал автор, и он часть предложения.
     * Удаление вместе со ссылкой рвало бы фразу в чужом файле. Ссылка без текста
     * (пустой якорь) исчезает целиком: сохранять там нечего.
     */
    fun removeLinksTo(id: String): Int {
        var changed = 0
        for (fileId in linkSourceIds().toList()) {
            if (fileId.equals(id, ignoreCase = true)) continue
            val text = files[fileId] ?: continue
            // один и тот же файл адресуется разными путями, поэтому сверяем разрешённую цель
            val updated = LINK_REGEX.replace(text) { m ->
                val target = tryResolveLink(fileId, m.groupValues[2])
                if (target != null && target.equals(id, ignoreCase = true)) m.groupValues[1] else m.value
            }
            if (updated != text) {
                writeFile(fileId, updated)
                changed++
            }
        }
        return changed
    }

    /**
     * Заголовок файла — его первая непустая строка. У файла в собственном формате
     * первая строка служебная (комментарий, ключ), поэтому там заголовок — имя файла.
     */
    fun title(id: String, maxLength: Int = 60): String {
        if (!isNeurodocFile(id)) return id.substringAfterLast('/')

        for (raw in readFile(id).lineSequence()) {
            val line = raw.trim().trimStart('#').trim()
            if (line.isEmpty()) continue
            return if (line.length > maxLength) line.take(maxLength).trimEnd() + "…" else line
        }
        return if (id == ROOT_ID) ROOT_ID else shorten(id)
    }

    /**
     * Все ссылки файла, включая неразрешимые. Список кэшируется и наружу отдаётся
     * только для чтения: он общий на всех вызывающих.
     */
    fun parsedLinks(sourceId: String): List<LinkRef> = linkCache.getOrPut(sourceId) {
        LINK_REGEX.findAll(readFile(sourceId)).map { m ->
            // регистр цели не трогаем: у пути он часть имени, а UUID приводит tryResolveLink
            val target = m.groupValues[2]
            LinkRef(
                start = m.range.first,
                length = m.value.length,
                target = target,
                anchor = m.groupValues[1],
                id = tryResolveLink(sourceId, target),
            )
        }.toList()
    }

    /**
     * Ссылки файла с уже разрешёнными целями. Цель зависит от того, где написана
     * ссылка, поэтому разбирать её в отрыве от файла-источника нельзя.
     */
    fun links(sourceId: String): List<LinkRef> = parsedLinks(sourceId).filter { it.id != null }

    /** Граф ссылок: id файла -> список id, на которые он ссылается. */
    fun buildGraph(): Map<String, List<String>> =
        linkSourceIds().associateWith { id ->
            parsedLinks(id).mapNotNull { it.id }.distinctBy { it.lowercase() }
        }

    /** Файлы, ссылающиеся на указанный. */
    fun backlinks(id: String): List<FileRef> =
        // Подстрокой больше не найти: один и тот же файл из разных папок адресуется
        // разными путями, поэтому ссылки каждого файла разрешаем по-настоящему.
        linkSourceIds()
            .filter { key ->
                !key.equals(id, ignoreCase = true) &&
                    parsedLinks(key).any { it.id != null && it.id.equals(id, ignoreCase = true) }
            }
            .map { FileRef(it, title(it)) }
            .sortedBy { it.title.lowercase() }

    /**
     * Ссылки на несуществующие файлы. Дубли допустимы: одна и та же цель
     * в разных местах — это разные записи, к каждой можно перейти.
     */
    fun brokenLinks(): List<BrokenLink> {
        val result = mutableListOf<BrokenLink>()
        val newlines = charArrayOf('\r', '\n')
        for (id in allIds().filter { isNeurodocFile(it) }) {
            var text: String? = null
            var fileTitle: String? = null
            for (link in parsedLinks(id)) {
                if (isWebTarget(link.target)) continue // веб-адрес файлом каталога не бывает
                // не разобралась — значит ведёт наружу каталога, это тоже битая ссылка
                val target = link.id ?: link.target
                if (exists(target)) continue

                // текст и заголовок нужны только файлам с битыми ссылками, а их единицы
                if (text == null) text = readFile(id)
                if (fileTitle == null) fileTitle = title(id, 40)

                val lineStart = if (link.start == 0) 0
                else text.lastIndexOfAny(newlines, link.start - 1).let { if (it < 0) 0 else it + 1 }
                val lineEnd = text.indexOfAny(newlines, link.start).let { if (it < 0) text.length else it }
                var context = text.substring(lineStart, lineEnd).trim()
                if (context.length > 70) context = context.take(70).trimEnd() + "…"

                result.add(BrokenLink(id, fileTitle, target, link.start, link.length, context))
            }
        }
        return result
    }

    /** Файлы (кроме root), на которые нет ни одной ссылки из других файлов. */
    fun orphanFiles(): List<FileRef> {
        val referenced = HashSet<String>()
        for (id in linkSourceIds()) {
            for (link in parsedLinks(id)) {
                val target = link.id ?: continue
                if (!target.equals(id, ignoreCase = true)) referenced.add(target.lowercase())
            }
        }
        // только neurodoc-файлы: файлы в собственных форматах живут своей жизнью,
        // и «сирота» среди них — не повод для тревоги
        return files.keys
            .filter { it != ROOT_ID && isNeurodocFile(it) && it.lowercase() !in referenced }
            .map { FileRef(it, title(it)) }
            .sortedBy { it.title.lowercase() }
    }

    /**
     * Кратчайшие пути от root до указанного файла — их показывают хлебные крошки.
     *
     * Сначала обход в ширину считает расстояния, и только потом перечисляются пути, где
     * каждый шаг приближает к цели ровно на единицу. Прежний обход в глубину с отсечкой
     * по числу найденных путей до короткого мог вообще не дойти: лимит выбирали длинные
     * пути, найденные раньше (проверено на Windows — вместо пути длиной 3 показывался
     * путь длиной 5).
     */
    fun shortestPathsFromRoot(target: String, maxPaths: Int = 50): List<List<String>> {
        if (target.equals(ROOT_ID, ignoreCase = true)) return emptyList()
        if (!exists(target)) return emptyList()

        val graph = buildGraph()
        // расстояние от каждого файла до цели (обход в ширину по обратным рёбрам)
        val incoming = HashMap<String, MutableList<String>>()
        for ((from, targets) in graph) {
            for (to in targets) incoming.getOrPut(to.lowercase()) { mutableListOf() }.add(from)
        }

        val distance = HashMap<String, Int>()
        distance[target.lowercase()] = 0
        val queue = ArrayDeque<String>()
        queue.add(target.lowercase())
        while (queue.isNotEmpty()) {
            val node = queue.removeFirst()
            val step = distance[node]!! + 1
            for (prev in incoming[node].orEmpty()) {
                val key = prev.lowercase()
                if (distance.containsKey(key)) continue
                distance[key] = step
                queue.add(key)
            }
        }

        val rootDistance = distance[ROOT_ID] ?: return emptyList()

        // перечисляем пути, где каждый шаг приближает к цели ровно на единицу
        val result = mutableListOf<List<String>>()
        val path = mutableListOf(ROOT_ID)

        fun walk(node: String, left: Int) {
            if (result.size >= maxPaths) return
            if (left == 0) {
                result.add(path.toList())
                return
            }
            for (next in graph[node].orEmpty()) {
                if (result.size >= maxPaths) return
                if (distance[next.lowercase()] != left - 1) continue
                path.add(next)
                walk(next, left - 1)
                path.removeAt(path.lastIndex)
            }
        }
        walk(ROOT_ID, rootDistance)
        return result
    }

    /**
     * Порядок чтения всего каталога: обход в глубину от root по порядку ссылок в тексте
     * (порядок ссылок — это замысел автора). Каждый файл встречается ровно один раз,
     * циклы отсекаются множеством пройденных.
     *
     * Хвостом идут файлы, до которых по ссылкам от root не добраться: без них «прочитать
     * всё» было бы недостижимо. Файлы в собственных форматах (контракты, конфиги) —
     * справочные данные, их читают по необходимости, а не подряд, поэтому в обход
     * они не входят.
     */
    fun buildReadingOrder(): List<String> {
        val order = mutableListOf<String>()
        val seen = HashSet<String>()

        // стек, а не рекурсия: глубина обхода равна длине цепочки ссылок
        val stack = ArrayDeque<String>()
        stack.addLast(ROOT_ID)
        while (stack.isNotEmpty()) {
            val id = stack.removeLast()
            if (!seen.add(id)) continue
            order.add(id)

            val children = mutableListOf<String>()
            for (link in parsedLinks(id)) {
                val target = link.id ?: continue
                if (!isNeurodocFile(target) || !exists(target) || target in seen) continue
                if (children.none { it.equals(target, ignoreCase = true) }) children.add(target)
            }
            // в стек — в обратном порядке, чтобы первым читался первый по тексту
            for (i in children.indices.reversed()) stack.addLast(children[i])
        }

        for (id in allIds()) {
            if (isNeurodocFile(id) && seen.add(id)) order.add(id)
        }
        return order
    }

    /** Поиск подстроки по всем файлам проекта (без учёта регистра). */
    fun search(query: String, maxResults: Int = 200): List<SearchHit> {
        val results = mutableListOf<SearchHit>()
        if (query.isEmpty()) return results

        for (id in allIds()) {
            val text = readFile(id)
            var index = 0
            var occurrence = 0
            while (results.size < maxResults) {
                index = text.indexOf(query, index, ignoreCase = true)
                if (index < 0) break
                val lineStart = if (index == 0) 0
                else text.lastIndexOfAny(charArrayOf('\r', '\n'), index - 1).let { if (it < 0) 0 else it + 1 }
                val matchEnd = index + query.length
                val lineEnd = text.indexOfAny(charArrayOf('\r', '\n'), matchEnd).let { if (it < 0) text.length else it }

                val prefixFrom = maxOf(lineStart, index - 60)
                val prefix = (if (prefixFrom > lineStart) "…" else "") + text.substring(prefixFrom, index)
                val suffixTo = minOf(lineEnd, matchEnd + 90)
                val suffix = text.substring(matchEnd, suffixTo) + (if (suffixTo < lineEnd) "…" else "")

                results.add(SearchHit(id, title(id, 40), prefix, text.substring(index, matchEnd), suffix, occurrence))
                occurrence++
                index += maxOf(1, query.length)
            }
            if (results.size >= maxResults) break
        }
        return results
    }

    /** Замена во всех файлах проекта. Возвращает число замен. */
    fun replaceAllInProject(query: String, replacement: String): Int {
        if (query.isEmpty()) return 0
        var total = 0
        for (id in allIds()) {
            val text = readFile(id)
            val count = countOccurrences(text, query)
            if (count == 0) continue
            writeFile(id, replaceIgnoreCase(text, query, replacement))
            total += count
        }
        return total
    }

    fun countOccurrences(text: String, query: String): Int {
        if (query.isEmpty()) return 0
        var count = 0
        var index = 0
        while (true) {
            index = text.indexOf(query, index, ignoreCase = true)
            if (index < 0) break
            count++
            index += query.length
        }
        return count
    }

    fun replaceIgnoreCase(text: String, query: String, replacement: String): String {
        if (query.isEmpty()) return text
        val sb = StringBuilder()
        var index = 0
        while (true) {
            val found = text.indexOf(query, index, ignoreCase = true)
            if (found < 0) {
                sb.append(text, index, text.length)
                break
            }
            sb.append(text, index, found).append(replacement)
            index = found + query.length
        }
        return sb.toString()
    }
}
