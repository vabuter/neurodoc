package today.vab.neurodoceditor.ui

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import today.vab.neurodoceditor.data.AppSettings
import today.vab.neurodoceditor.data.BrokenLink
import today.vab.neurodoceditor.data.CatalogMigration
import today.vab.neurodoceditor.data.FileRef
import today.vab.neurodoceditor.data.GitCommitInfo
import today.vab.neurodoceditor.data.GitException
import today.vab.neurodoceditor.data.GitService
import today.vab.neurodoceditor.data.LegacyScan
import today.vab.neurodoceditor.data.MigrationResult
import today.vab.neurodoceditor.data.NeurodocCatalog
import today.vab.neurodoceditor.data.ProjectInfo
import today.vab.neurodoceditor.data.PullOutcome
import today.vab.neurodoceditor.data.PushOutcome
import today.vab.neurodoceditor.data.SyncOutcome
import today.vab.neurodoceditor.data.ReadingProgress
import today.vab.neurodoceditor.data.ReadingProgressStore
import today.vab.neurodoceditor.data.SearchHit
import today.vab.neurodoceditor.data.SettingsStore
import java.io.File

/** Режим просмотра содержимого — как кнопки форматов в Windows-версии. */
enum class ViewMode { TEXT, SVG, JSON, YAML, BASH, MD, ADOC, HTML, NGINX }

/**
 * Где пользователь был в файле: каретка, прокрутка и режим просмотра.
 * Режим лежит здесь же, поэтому при возврате он ПОБЕЖДАЕТ определение по расширению:
 * выключенный вручную рендер у .md не включается заново, а включённый вручную MD
 * у файла без расширения — восстанавливается.
 */
private data class ViewState(val caret: Int, val scroll: Int, val mode: ViewMode)

class EditorViewModel(app: Application) : AndroidViewModel(app) {

    private val store = SettingsStore(app)
    private val readingStore = ReadingProgressStore(app)
    private val backupsDir = File(app.filesDir, "backups")

    var settings by mutableStateOf(store.load())
        private set

    var catalog by mutableStateOf<NeurodocCatalog?>(null)
        private set

    var currentId by mutableStateOf<String?>(null)
        private set

    var text by mutableStateOf(TextFieldValue(""))
        private set

    var viewMode by mutableStateOf(ViewMode.TEXT)
        private set

    /** Пути от root до текущего файла (кратчайшие). */
    val breadcrumbs = mutableStateListOf<List<FileRef>>()
    val backlinks = mutableStateListOf<FileRef>()
    val orphans = mutableStateListOf<FileRef>()

    /** Битые ссылки проекта: ссылка есть, а файла нет. */
    val brokenLinks = mutableStateListOf<BrokenLink>()

    /** Результаты поиска по проекту. */
    val searchResults = mutableStateListOf<SearchHit>()
    var searchQuery by mutableStateOf("")
        private set

    /** Поиск по текущему файлу. */
    var findQuery by mutableStateOf("")
        private set
    var findMatches by mutableStateOf<List<IntRange>>(emptyList())
        private set
    var findCurrent by mutableStateOf(-1)
        private set

    var statusMessage by mutableStateOf<String?>(null)

    /** Прогресс чтения текущего проекта. */
    private var reading by mutableStateOf(ReadingProgress())
    var readCount by mutableStateOf(0)
        private set
    var readTotal by mutableStateOf(0)
        private set

    /** Каталог просит перевода на новый формат; null — просить нечего. */
    var pendingMigration by mutableStateOf<LegacyScan?>(null)
        private set

    /** Прокрутка, которую надо вернуть при показе файла; -1 — возвращать нечего. */
    var pendingScroll by mutableStateOf(-1)
        private set

    private val backStack = mutableStateListOf<String>()
    private val forwardStack = mutableStateListOf<String>()
    private val viewStates = HashMap<String, ViewState>()
    /**
     * Последнее непустое выделение. Действия вызывают из меню, а его открытие уводит
     * фокус из поля, и Compose схлопывает выделение — к моменту нажатия «Связать»
     * выделять уже нечего. В Windows-версии стоит ровно такая же защита.
     */
    private var lastSelection: IntRange? = null
    private var lastSavedText = ""
    private var saveJob: Job? = null
    private var readJob: Job? = null
    private var currentScroll = 0

    val canGoBack: Boolean get() = canNavigate(backStack)
    val canGoForward: Boolean get() = canNavigate(forwardStack)
    val isRoot: Boolean get() = currentId == NeurodocCatalog.ROOT_ID

    /** Файл формата neurodoc — только такие входят в чтение и отмечаются прочитанными. */
    val isNeurodocFile: Boolean get() = currentId?.let { NeurodocCatalog.isNeurodocFile(it) } == true

    val isCurrentRead: Boolean get() = currentId?.let { reading.isRead(it) } == true

    /**
     * Есть ли куда идти по кнопке «Далее».
     *
     * Пока текущий файл не отмечен прочитанным, шагать некуда: «Далее» — это шаг
     * вперёд по порядку чтения, а уходить из неотмеченного файла значит оставлять
     * дырку, к которой придётся возвращаться. Файлы в собственных форматах в обход
     * чтения не входят и ничего не задерживают.
     */
    val hasNextUnread: Boolean
        get() {
            val id = currentId ?: return false
            if (NeurodocCatalog.isNeurodocFile(id) && !reading.isRead(id)) return false
            return nextUnreadFile() != null
        }

    /**
     * Есть ли в истории файл, который ещё существует. Считать записи мало: переход
     * пропускает исчезнувшие файлы, и стопка из одних таких оставляла бы кнопку
     * доступной, а нажатие — без всякого действия.
     */
    private fun canNavigate(history: List<String>): Boolean {
        val cat = catalog ?: return false
        return history.any { cat.exists(it) }
    }

    /** У пути регистр — часть имени, поэтому к нижнему регистру приводим только UUID. */
    private fun normalizeId(id: String): String =
        if (NeurodocCatalog.isUuid(id)) id.lowercase() else id

    // ================= Проекты =================

    fun openProject(project: ProjectInfo) {
        saveNow()
        backStack.clear()
        forwardStack.clear()
        viewStates.clear()
        currentId = null
        pendingMigration = null
        runCatching { NeurodocCatalog.openOrCreate(project.path) }
            .onSuccess { catalog = it }
            .onFailure {
                catalog = null
                statusMessage = "Не удалось открыть проект: ${it.message}"
            }
        reading = readingStore.load(project.id)
        updateSettings(settings.copy(currentProjectId = project.id))
        setupGit()
        catalog?.let { cat ->
            openFile(NeurodocCatalog.ROOT_ID, push = false)
            // предложение о миграции — после открытия: пользователь уже видит каталог
            val scan = runCatching { CatalogMigration.scan(cat) }.getOrNull()
            if (scan != null && scan.needsMigration) pendingMigration = scan
        }
    }

    /**
     * Тёмный ли фон у просмотра SVG: выбор пользователя для этого проекта, а пока его
     * нет — тема приложения.
     */
    fun svgBackgroundDark(themeIsDark: Boolean): Boolean =
        when (settings.currentProject?.svgBackground) {
            "Dark" -> true
            "Light" -> false
            else -> themeIsDark
        }

    /** Запоминает выбранный в просмотре фон за текущим проектом. */
    fun saveSvgBackground(dark: Boolean) {
        val current = settings.currentProject ?: return
        val value = if (dark) "Dark" else "Light"
        if (current.svgBackground == value) return
        val projects = settings.projects.map {
            if (it.id == current.id) it.copy(svgBackground = value) else it
        }
        updateSettings(settings.copy(projects = projects))
    }

    fun updateSettings(newSettings: AppSettings) {
        settings = newSettings
        store.save(newSettings)
    }

    fun addOrUpdateProject(project: ProjectInfo) {
        val projects = settings.projects.toMutableList()
        val index = projects.indexOfFirst { it.id == project.id }
        if (index >= 0) projects[index] = project else projects.add(project)
        updateSettings(
            settings.copy(
                projects = projects,
                currentProjectId = settings.currentProjectId ?: project.id,
            )
        )
    }

    fun deleteProject(project: ProjectInfo) {
        val projects = settings.projects.filterNot { it.id == project.id }
        val nextCurrent =
            if (settings.currentProjectId == project.id) projects.firstOrNull()?.id
            else settings.currentProjectId
        updateSettings(settings.copy(projects = projects, currentProjectId = nextCurrent))
    }

    // ================= Миграция каталога =================

    fun dismissMigration() {
        pendingMigration = null
    }

    /** Переводит каталог на новый формат — только по явному согласию пользователя. */
    fun runMigration(): MigrationResult? {
        val cat = catalog ?: return null
        pendingMigration = null
        return runCatching { CatalogMigration.migrate(cat, backupsDir, settings.currentProjectId) }
            .onSuccess { result ->
                cat.reload()
                reloadCurrentFromDisk()
                refreshPanels()
                statusMessage =
                    "Ссылок переведено: ${result.links} в ${result.files} файлах. " +
                        "Копия каталога: ${result.backupPath}"
            }
            .onFailure { statusMessage = "Миграция не выполнена: ${it.message}" }
            .getOrNull()
    }

    // ================= Навигация =================

    fun openFile(id: String, push: Boolean = true) {
        val cat = catalog ?: return
        val target = normalizeId(id)
        if (currentId == target) return

        saveNow()
        rememberViewState()
        if (push && currentId != null) {
            backStack.add(currentId!!)
            forwardStack.clear() // как в браузере
        }
        currentId = target
        lastSelection = null

        val content = cat.readFile(target)
        val state = viewStates[target]
        val caret = (state?.caret ?: 0).coerceIn(0, content.length)
        text = TextFieldValue(content, TextRange(caret))
        lastSavedText = content
        pendingScroll = state?.scroll ?: 0
        currentScroll = pendingScroll

        // Расширение решает только при первом заходе: запомненный режим побеждает,
        // иначе выключенный вручную рендер у .md включался бы заново.
        viewMode = state?.mode ?: detectViewMode(target)

        scheduleReadMark()
        refreshPanels()
        refreshFindMatches()
    }

    /** Прокрутка редактора — её запоминаем, чтобы вернуть при возврате в файл. */
    fun onScrollChanged(value: Int) {
        currentScroll = value
    }

    fun scrollRestored() {
        pendingScroll = -1
    }

    private fun rememberViewState() {
        val id = currentId ?: return
        viewStates[id] = ViewState(text.selection.start, currentScroll, viewMode)
    }

    /**
     * Формат файла по расширению — чтобы файл в собственном формате открывался сразу
     * в подходящем виде. У root и файлов _content расширения нет, для них это TEXT.
     */
    private fun detectViewMode(id: String): ViewMode =
        when (id.substringAfterLast('.', "").lowercase()) {
            "svg" -> ViewMode.SVG
            "json" -> ViewMode.JSON
            "yaml", "yml" -> ViewMode.YAML
            "sh", "bash" -> ViewMode.BASH
            "md", "markdown" -> ViewMode.MD
            "adoc", "asciidoc" -> ViewMode.ADOC
            "html", "htm" -> ViewMode.HTML
            "conf", "nginx" -> ViewMode.NGINX
            else -> ViewMode.TEXT
        }

    /**
     * Шаг по истории: снимаем файл с одной стопки, текущий кладём на другую.
     * Исчезнувшие файлы пропускаем — их могли удалить другой программой.
     */
    private fun goHistory(from: MutableList<String>, to: MutableList<String>) {
        val cat = catalog ?: return
        while (from.isNotEmpty()) {
            val id = from.removeAt(from.lastIndex)
            if (!cat.exists(id)) continue
            currentId?.let { to.add(it) }
            openFile(id, push = false)
            break
        }
    }

    fun goBack() = goHistory(backStack, forwardStack)

    fun goForward() = goHistory(forwardStack, backStack)

    fun goHome() = openFile(NeurodocCatalog.ROOT_ID)

    /** Перечитывает каталог с диска, подхватывая правки других программ (git, редактор). */
    fun reloadFromDisk() {
        val cat = catalog ?: return
        saveNow()
        cat.reload()
        reloadCurrentFromDisk()
        refreshPanels()
        refreshFindMatches()
        statusMessage = "Проект перечитан"
    }

    private fun reloadCurrentFromDisk() {
        val cat = catalog ?: return
        val id = currentId ?: return
        val onDisk = cat.readFile(id)
        // свои несохранённые правки чужим содержимым не затираем
        if (text.text != lastSavedText || onDisk == lastSavedText) return
        val caret = text.selection.start.coerceIn(0, onDisk.length)
        text = TextFieldValue(onDisk, TextRange(caret))
        lastSavedText = onDisk
    }

    // ================= Редактирование =================

    fun onTextChange(value: TextFieldValue) {
        val textChanged = value.text != text.text
        text = value
        val from = minOf(value.selection.start, value.selection.end)
        val to = maxOf(value.selection.start, value.selection.end)
        if (to > from) lastSelection = from until to
        if (textChanged) {
            // правка сдвигает позиции — запомненное выделение больше не про этот текст
            lastSelection = if (to > from) from until to else null
            refreshFindMatches()
            scheduleSave()
        }
    }

    private fun scheduleSave() {
        saveJob?.cancel()
        saveJob = viewModelScope.launch {
            delay(700)
            saveNow()
        }
    }

    fun saveNow() {
        val cat = catalog ?: return
        val id = currentId ?: return
        if (text.text == lastSavedText) return
        runCatching { cat.writeFile(id, text.text) }
            .onSuccess {
                lastSavedText = text.text
                refreshPanels()
            }
            .onFailure { statusMessage = "Не удалось сохранить: ${it.message}" }
    }

    /**
     * Включает или выключает режим «как есть».
     *
     * Это такой же режим показа, как SVG или MD, поэтому включение выводит из
     * отрисованного формата: иначе два режима были бы включены разом.
     */
    fun setRawEditing(raw: Boolean) {
        updateSettings(settings.copy(rawEditing = raw))
        if (raw && viewMode != ViewMode.TEXT) changeViewMode(ViewMode.TEXT)
    }

    /**
     * Приводит текст к пригодному для якоря виду: он однострочный и не содержит "]",
     * иначе ссылка перестанет разбираться.
     */
    private fun sanitizeAnchor(text: String): String =
        text.replace('\r', ' ').replace('\n', ' ').replace("]", "").trim()

    /**
     * Выделение, с которым работают действия из меню.
     *
     * Если текущее выделение схлопнуто (фокус ушёл на кнопку меню), берём последнее
     * непустое — но только если оно всё ещё укладывается в текст.
     */
    private fun actionSelection(): IntRange? {
        val length = text.text.length
        val from = minOf(text.selection.start, text.selection.end).coerceIn(0, length)
        val to = maxOf(text.selection.start, text.selection.end).coerceIn(0, length)
        if (to > from) return from until to

        val remembered = lastSelection ?: return null
        if (remembered.first < 0 || remembered.last + 1 > length) return null
        return remembered
    }

    /**
     * Ссылки в тексте РЕДАКТОРА, а не в сохранённой копии каталога: автосохранение
     * приходит с задержкой, и до него позиции ссылок в каталоге уже не те.
     */
    private fun editorLinks(): List<TextLink> {
        val id = currentId ?: return emptyList()
        return findLinks(text.text, id)
    }

    /** Ссылка, накрывающая позицию (границы включительно). */
    private fun linkAt(position: Int): TextLink? =
        editorLinks().firstOrNull { position >= it.range.first && position <= it.range.last + 1 }

    /** Пересекает ли диапазон хоть одну ссылку — внутрь ссылки писать нельзя. */
    private fun overlapsLink(from: Int, to: Int): Boolean =
        editorLinks().any { from < it.range.last + 1 && to > it.range.first }

    /** Границы слова под кареткой; пусто — каретка не на слове. */
    private fun wordAt(position: Int): IntRange? {
        val content = text.text
        fun isWordChar(c: Char) = c.isLetterOrDigit() || c == '_' || c == '-'
        var start = position.coerceIn(0, content.length)
        var end = start
        while (start > 0 && isWordChar(content[start - 1])) start--
        while (end < content.length && isWordChar(content[end])) end++
        return if (end > start) start until end else null
    }

    /**
     * Вставляет новую ссылку, создаёт файл и переходит к нему.
     *
     * Якорем становится выделенный текст, а без выделения — слово под кареткой:
     * пользователь чаще всего хочет превратить в ссылку именно его.
     */
    fun insertNewLink() {
        val cat = catalog ?: return
        val id = currentId ?: return
        if (!NeurodocCatalog.isNeurodocFile(id)) {
            statusMessage = "Ссылки ставят только в файлах neurodoc"
            return
        }
        val selection = actionSelection()
        var start = selection?.first ?: text.selection.start.coerceIn(0, text.text.length)
        var end = selection?.let { it.last + 1 } ?: start

        // внутрь существующей ссылки вставлять нельзя — она перестанет разбираться
        if (overlapsLink(start, end) || (start == end && linkAt(start) != null)) {
            statusMessage = "Каретка внутри ссылки — сначала выйдите из неё"
            return
        }
        // без выделения якорем берём слово под кареткой
        if (start == end) {
            wordAt(start)?.let { word ->
                start = word.first
                end = word.last + 1
            }
        }

        val anchor = sanitizeAnchor(text.text.substring(start, end))
        val newId = cat.createNewFile()
        if (anchor.isNotEmpty()) cat.writeFile(newId, anchor + "\n")

        val link = "[$anchor]($newId)"
        val updated = text.text.replaceRange(start, end, link)
        text = TextFieldValue(updated, TextRange(start + link.length))
        saveNow()
        openFile(newId)
    }

    /**
     * Связывает выделенный текст со ссылкой из буфера обмена.
     *
     * @return сообщение об отказе или null, если связали.
     */
    fun linkSelection(clipboard: String?): String? {
        val cat = catalog ?: return "Проект не открыт"
        val id = currentId ?: return "Файл не открыт"
        if (!NeurodocCatalog.isNeurodocFile(id)) return "Ссылки ставят только в файлах neurodoc"

        val selection = actionSelection() ?: return "Выделите текст, который станет ссылкой"
        val start = selection.first
        val end = selection.last + 1
        if (overlapsLink(start, end)) return "Выделение задевает существующую ссылку"

        // подсказываем, чего именно не хватает: «ничего не вышло» пользователю бесполезно
        val raw = clipboard?.trim().orEmpty()
        if (raw.isEmpty()) return "Буфер обмена пуст — сначала «Копировать ссылку» в нужном файле"
        val target = targetFromClipboard(clipboard, cat)
            ?: return "В буфере обмена не ссылка на файл этого проекта, а «" +
                raw.take(40) + (if (raw.length > 40) "…" else "") + "»"

        val anchor = sanitizeAnchor(text.text.substring(start, end))
        if (anchor.isEmpty()) return "Выделен только пробельный текст"

        // цель записываем относительно ТОГО файла, в котором пишем ссылку
        val relative = NeurodocCatalog.makeRelativeTarget(NeurodocCatalog.dirOf(id), target)
        val link = "[$anchor]($relative)"
        text = TextFieldValue(text.text.replaceRange(start, end, link), TextRange(start + link.length))
        saveNow()
        return null
    }

    /**
     * Достаёт из буфера обмена id файла ЭТОГО каталога.
     *
     * Проверка строгая: без неё в текст попадала ссылка на несуществующий файл —
     * произвольный текст из буфера принимался за путь (проверено на Windows).
     */
    private fun targetFromClipboard(clipboard: String?, cat: NeurodocCatalog): String? {
        val raw = clipboard?.trim().orEmpty()
        if (raw.isEmpty()) return null
        // ссылка целиком: [текст](цель)
        val inside = NeurodocCatalog.LINK_REGEX.find(raw)?.groupValues?.get(2) ?: raw
        if (inside.any { it.isWhitespace() }) return null
        val id = cat.resolveInput(inside) ?: return null
        return if (cat.exists(id)) id else null
    }

    /** Снимает ссылку под кареткой, оставляя её текст. */
    fun removeLinkAtCaret(): Boolean {
        currentId ?: return false
        val link = linkAt(text.selection.start) ?: return false
        // веб-адрес ссылкой формата не является — снимать с него нечего
        if (link.isWeb && link.anchor.isEmpty()) return false
        val start = link.range.first
        val end = link.range.last + 1
        val anchor = link.anchor
        text = TextFieldValue(
            text.text.replaceRange(start, end, anchor),
            TextRange(start + anchor.length),
        )
        saveNow()
        return true
    }

    /** Ссылка, на которой стоит каретка, — для перехода по ней из меню. */
    fun linkTargetAtCaret(): String? = linkAt(text.selection.start)?.takeIf { !it.isWeb }?.target

    fun createUnlinkedFile() {
        val cat = catalog ?: return
        openFile(cat.createNewFile())
    }

    /** Ссылка на текущий файл в виде, пригодном для вставки в файл из _content. */
    fun currentFileLink(): String? {
        val cat = catalog ?: return null
        val id = currentId ?: return null
        if (id == NeurodocCatalog.ROOT_ID) return null
        // якорем берём заголовок файла; у пустого файла заголовка нет — оставляем пустой
        val anchor = if (cat.readFile(id).isBlank()) "" else sanitizeAnchor(cat.title(id))
        // Путь считается от файла, куда ссылку вставят, а этого приложение знать не может.
        // Берём вид для _content: там пишут ссылки в подавляющем большинстве случаев.
        val target = NeurodocCatalog.makeRelativeTarget(NeurodocCatalog.CONTENT_DIR, id)
        return "[$anchor]($target)"
    }

    fun deleteCurrentFile(alsoRemoveLinks: Boolean) {
        val cat = catalog ?: return
        val id = currentId ?: return
        if (id == NeurodocCatalog.ROOT_ID) return
        saveJob?.cancel()
        readJob?.cancel()
        currentId = null
        lastSavedText = ""
        runCatching {
            cat.deleteFile(id)
            if (alsoRemoveLinks) cat.removeLinksTo(id)
        }.onFailure { statusMessage = "Не удалось удалить: ${it.message}" }
        viewStates.remove(id)
        backStack.removeAll { it == id }
        forwardStack.removeAll { it == id }
        openFile(NeurodocCatalog.ROOT_ID, push = false)
    }

    // ================= Прогресс чтения =================

    /** Отметка о прочтении ставится не сразу: промах по ссылке не должен её ставить. */
    private fun scheduleReadMark() {
        readJob?.cancel()
        if (!isNeurodocFile) return
        readJob = viewModelScope.launch {
            delay(3000)
            markCurrentRead(true)
        }
    }

    fun markCurrentRead(read: Boolean) {
        val projectId = settings.currentProjectId ?: return
        val id = currentId ?: return
        // файлы в собственных форматах в обход чтения не входят — и отмечать их нечего
        if (!NeurodocCatalog.isNeurodocFile(id)) return
        if (read == reading.isRead(id)) return

        readJob?.cancel()
        reading.mark(id, read)
        readingStore.save(projectId, reading)
        refreshReadingCounters()
        // ссылки на этот файл в текущем тексте должны перекраситься сразу
        text = text.copy()
    }

    fun toggleCurrentRead() = markCurrentRead(!isCurrentRead)

    fun isRead(id: String): Boolean = reading.isRead(id)

    /**
     * Следующий непрочитанный файл в порядке обхода, или null — идти некуда.
     *
     * Текущий файл не в счёт, даже если он ещё не отмечен прочитанным: переход к самому
     * себе openFile отсекает, и кнопка выглядела бы рабочей впустую.
     */
    private fun nextUnreadFile(order: List<String>? = null): String? {
        val cat = catalog ?: return null
        for (id in order ?: cat.buildReadingOrder()) {
            if (reading.isRead(id)) continue
            if (id.equals(currentId, ignoreCase = true)) continue
            return id
        }
        return null
    }

    /** Открывает следующий непрочитанный файл в порядке обхода. */
    fun openNextUnread() {
        nextUnreadFile()?.let { openFile(it) }
            ?: run { statusMessage = "Непрочитанных файлов нет" }
    }

    fun resetReadingProgress() {
        val projectId = settings.currentProjectId ?: return
        readingStore.reset(projectId)
        reading = ReadingProgress()
        refreshReadingCounters()
        text = text.copy()
        statusMessage = "Прогресс чтения сброшен"
    }

    private fun refreshReadingCounters() {
        val cat = catalog
        if (cat == null) {
            readCount = 0
            readTotal = 0
            return
        }
        val order = cat.buildReadingOrder()
        readTotal = order.size
        readCount = order.count { reading.isRead(it) }
    }

    // ================= Панели =================

    private fun refreshPanels() {
        val cat = catalog ?: return
        val id = currentId ?: return
        breadcrumbs.clear()
        breadcrumbs.addAll(
            cat.shortestPathsFromRoot(id).map { path ->
                path.map { FileRef(it, if (it == NeurodocCatalog.ROOT_ID) "root" else cat.title(it, 36)) }
            }
        )
        backlinks.clear()
        backlinks.addAll(cat.backlinks(id))
        orphans.clear()
        orphans.addAll(cat.orphanFiles())
        brokenLinks.clear()
        brokenLinks.addAll(cat.brokenLinks())
        refreshReadingCounters()
    }

    // ================= Поиск =================

    fun runProjectSearch(query: String) {
        val cat = catalog ?: return
        searchQuery = query
        searchResults.clear()
        if (query.isNotBlank()) searchResults.addAll(cat.search(query))
    }

    fun clearProjectSearch() {
        searchQuery = ""
        searchResults.clear()
    }

    fun openSearchHit(hit: SearchHit) {
        openFile(hit.fileId)
        val content = text.text
        var index = -1
        var from = 0
        for (i in 0..hit.occurrence) {
            index = content.indexOf(searchQuery, from, ignoreCase = true)
            if (index < 0) return
            from = index + maxOf(1, searchQuery.length)
        }
        if (index >= 0) {
            text = text.copy(selection = TextRange(index, index + searchQuery.length))
        }
    }

    fun updateFindQuery(query: String) {
        findQuery = query
        refreshFindMatches()
        if (findMatches.isNotEmpty()) {
            findCurrent = 0
            selectFindMatch()
        } else {
            findCurrent = -1
        }
    }

    private fun refreshFindMatches() {
        val query = findQuery
        if (query.isEmpty()) {
            findMatches = emptyList()
            findCurrent = -1
            return
        }
        val matches = mutableListOf<IntRange>()
        var index = 0
        val content = text.text
        while (true) {
            index = content.indexOf(query, index, ignoreCase = true)
            if (index < 0) break
            matches.add(index until index + query.length)
            index += maxOf(1, query.length)
        }
        findMatches = matches
        if (findCurrent >= matches.size) findCurrent = if (matches.isEmpty()) -1 else 0
    }

    /** Открывает файл и выделяет в нём место битой ссылки. */
    fun openBrokenLink(link: BrokenLink) {
        openFile(link.fileId)
        val end = (link.position + link.length).coerceAtMost(text.text.length)
        if (link.position in 0..end) {
            text = text.copy(selection = TextRange(link.position, end))
        }
    }

    fun findStep(forward: Boolean) {
        if (findMatches.isEmpty()) return
        findCurrent = when {
            findCurrent < 0 -> 0
            forward -> (findCurrent + 1) % findMatches.size
            else -> (findCurrent - 1 + findMatches.size) % findMatches.size
        }
        selectFindMatch()
    }

    private fun selectFindMatch() {
        val range = findMatches.getOrNull(findCurrent) ?: return
        text = text.copy(selection = TextRange(range.first, range.last + 1))
    }

    fun closeFind() {
        findQuery = ""
        findMatches = emptyList()
        findCurrent = -1
    }

    // ================= Замена =================

    /** Заменяет одно вхождение в текущем файле (от каретки). true — заменено. */
    fun replaceOneInCurrentFile(query: String, replacement: String): Boolean {
        if (query.isEmpty() || currentId == null) return false
        val content = text.text
        val from = text.selection.start.coerceIn(0, content.length)
        var index = content.indexOf(query, from, ignoreCase = true)
        if (index < 0) index = content.indexOf(query, 0, ignoreCase = true)
        if (index < 0) return false
        val updated = content.replaceRange(index, index + query.length, replacement)
        text = TextFieldValue(updated, TextRange(index + replacement.length))
        saveNow()
        refreshFindMatches()
        return true
    }

    fun replaceAllInCurrentFile(query: String, replacement: String): Int {
        val cat = catalog ?: return 0
        if (query.isEmpty() || currentId == null) return 0
        val content = text.text
        val count = cat.countOccurrences(content, query)
        if (count == 0) return 0
        text = TextFieldValue(cat.replaceIgnoreCase(content, query, replacement), TextRange(0))
        saveNow()
        refreshFindMatches()
        return count
    }

    fun replaceAllInProject(query: String, replacement: String): Int {
        val cat = catalog ?: return 0
        saveNow()
        val total = cat.replaceAllInProject(query, replacement)
        currentId?.let { id ->
            val content = cat.readFile(id)
            text = TextFieldValue(content, TextRange(0))
            lastSavedText = content
        }
        refreshPanels()
        refreshFindMatches()
        return total
    }

    // ================= Просмотр форматов =================

    fun changeViewMode(mode: ViewMode) {
        // выбор формата снимает «как есть» — это режимы одного ряда, см. setRawEditing
        if (mode != ViewMode.TEXT && settings.rawEditing) {
            updateSettings(settings.copy(rawEditing = false))
        }
        if (mode != ViewMode.TEXT) saveNow()
        viewMode = mode
        gitDiff = null // дельта — разовый просмотр, выбор формата её снимает
    }

    // ================= Git =================

    var git by mutableStateOf<GitService?>(null)
        private set
    var gitBranch by mutableStateOf<String?>(null)
        private set
    var gitBranches by mutableStateOf<List<String>>(emptyList())
        private set
    var gitDefaultBranch by mutableStateOf<String?>(null)
        private set

    /** Идёт git-операция: индикатор в шапке, кнопки git-диалога выключены. */
    var gitBusy by mutableStateOf(false)
        private set

    /** Ошибка git — показывается отдельным диалогом: в Toast её текст не помещается. */
    var gitError by mutableStateOf<String?>(null)

    /** Какой конфликт ждёт решения пользователя; управляет диалогами в MainActivity. */
    var gitConflict by mutableStateOf<GitConflictKind?>(null)

    var gitHistoryVisible by mutableStateOf(false)
    var gitCommits by mutableStateOf<List<GitCommitInfo>>(emptyList())
        private set
    var gitSelectedCommit by mutableStateOf<GitCommitInfo?>(null)
        private set
    var gitFiles by mutableStateOf<List<GitFileRow>>(emptyList())
        private set
    var gitSinceMode by mutableStateOf(false)
        private set

    /** Показанная дельта файла (unified diff) и её заголовок; null — дельта не показана. */
    var gitDiff by mutableStateOf<Pair<String, String>?>(null)

    /** Пересоздаёт сервис при открытии проекта — зовётся из openProject. */
    private fun setupGit() {
        val path = settings.currentProject?.path
        git = if (path != null && GitService.isRepository(File(path))) makeGit(File(path)) else null
        gitHistoryVisible = false
        gitCommits = emptyList()
        gitSelectedCommit = null
        gitDiff = null
        refreshGitState()
    }

    private fun makeGit(dir: File) = GitService(
        dir = dir,
        credentials = { url -> GitService.credentialsProvider(url, settings.gitAccounts) },
        identity = {
            val name = settings.gitUserName.ifBlank { android.os.Build.MODEL }
            val email = settings.gitUserEmail
            if (email.isBlank()) {
                throw GitException("Заполните имя и почту автора в настройках Git.")
            }
            org.eclipse.jgit.lib.PersonIdent(name, email)
        },
    )

    /** Обновляет ветку и их список. После каждой git-операции. */
    private fun refreshGitState() {
        val g = git ?: run {
            gitBranch = null
            gitBranches = emptyList()
            gitDefaultBranch = null
            return
        }
        viewModelScope.launch {
            runCatching {
                gitBranch = g.currentBranch()
                gitBranches = g.localBranches()
                gitDefaultBranch = g.defaultBranch()
            }
            if (gitHistoryVisible) loadGitCommits()
        }
    }

    /**
     * Каркас каждой git-операции: сохранить правки, занятость, выполнить, перечитать
     * каталог, обновить ветку. Ошибки git всегда показываются пользователю.
     */
    private fun runGit(block: suspend (GitService) -> Unit) {
        val g = git ?: return
        if (gitBusy) return
        saveJob?.cancel()
        saveNow()
        gitBusy = true
        viewModelScope.launch {
            try {
                block(g)
            } catch (e: Exception) {
                gitError = e.message ?: e.toString()
            } finally {
                gitBusy = false
            }
            // слежения за диском на Android нет — перечитываем каталог сами
            catalog?.reload()
            reloadCurrentFromDisk()
            refreshPanels()
            refreshFindMatches()
            ensureCurrentExists()
            refreshGitState()
        }
    }

    /** После pull/checkout/reset текущий файл мог исчезнуть — возвращаемся в root. */
    private fun ensureCurrentExists() {
        val cat = catalog ?: return
        val id = currentId ?: return
        if (cat.exists(id)) return
        currentId = null
        openFile(NeurodocCatalog.ROOT_ID, push = false)
        statusMessage = "Файл исчез после операции git — открыт root"
    }

    // -------- истории 5–7: сохранить, получить, синхронизировать --------

    fun gitSave() = runGit { g ->
        when (g.commitAndPush(GitService.buildCommitMessage())) {
            PushOutcome.OK -> statusMessage = "Сохранено на сервере"
            PushOutcome.REJECTED_NON_FAST_FORWARD -> gitConflict = GitConflictKind.PUSH
        }
    }

    fun gitPull() = runGit { g ->
        when (g.pull()) {
            PullOutcome.OK -> statusMessage = "Получено с сервера"
            PullOutcome.CONFLICT -> gitConflict = GitConflictKind.PULL
        }
    }

    fun gitSync() = runGit { g ->
        when (g.sync(GitService.buildCommitMessage())) {
            SyncOutcome.OK -> statusMessage = "Синхронизировано"
            SyncOutcome.CONFLICT -> gitConflict = GitConflictKind.SYNC
            SyncOutcome.PUSH_REJECTED -> gitConflict = GitConflictKind.PUSH
        }
    }

    /** Решение конфликта: взять серверную версию (свои изменения затираются). */
    fun gitResolveTakeServer() = runGit { g ->
        g.resetToRemote()
        statusMessage = "Взята серверная версия"
    }

    /** Решение конфликта: затереть серверную версию своей. */
    fun gitResolveTakeMine() = runGit { g ->
        g.forcePush()
        statusMessage = "Серверная версия перезаписана вашей"
    }

    // -------- истории 3–4: ветки --------

    fun gitSwitchBranch(name: String) = runGit { g ->
        g.switchBranch(name)
        statusMessage = "Ветка переключена: $name"
    }

    fun gitCreateBranch(name: String) = runGit { g ->
        g.createBranch(name)
        statusMessage = "Ветка создана и отправлена: $name"
    }

    fun gitDeleteBranch(name: String) = runGit { g ->
        if (name == gitBranch) g.switchBranch(g.defaultBranch())
        g.deleteBranch(name)
        statusMessage = "Ветка удалена: $name"
    }

    // -------- истории 1–2: опубликовать, клонировать --------

    fun gitPublish(url: String) {
        val path = settings.currentProject?.path ?: return
        if (GitService.isRepository(File(path))) {
            gitError = "Каталог уже является git-репозиторием."
            return
        }
        // публикация делает каталог репозиторием — пересоздаём сервис до раннера
        git = makeGit(File(path))
        runGit { g ->
            try {
                g.initAndPublish(url)
                statusMessage = "Проект опубликован"
            } catch (e: Exception) {
                if (!GitService.isRepository(File(path))) git = null
                throw e
            }
        }
    }

    /** Клонирование: папку чистит и наполняет GitService, затем проект открывается как обычно. */
    fun gitClone(url: String, name: String, path: String) {
        if (gitBusy) return
        gitBusy = true
        viewModelScope.launch {
            try {
                GitService.clone(
                    url, File(path),
                    GitService.credentialsProvider(url, settings.gitAccounts),
                )
                val project = ProjectInfo(name = name, path = path)
                addOrUpdateProject(project)
                openProject(project)
                statusMessage = "Клонировано: $name"
            } catch (e: Exception) {
                gitError = e.message ?: e.toString()
            } finally {
                gitBusy = false
            }
        }
    }

    // -------- истории 8–9: история и дельты --------

    fun gitShowHistory() {
        gitHistoryVisible = true
        gitSelectedCommit = null
        gitSinceMode = false
        viewModelScope.launch { loadGitCommits() }
    }

    fun gitHideHistory() {
        gitHistoryVisible = false
        gitDiff = null
    }

    fun gitBackToCommits() {
        gitSelectedCommit = null
        gitFiles = emptyList()
    }

    private suspend fun loadGitCommits() {
        val g = git ?: return
        runCatching { gitCommits = g.history() }
            .onFailure { gitError = it.message }
        // выбранный коммит после перечитывания мог исчезнуть — возвращаемся к списку
        if (gitSelectedCommit != null && gitCommits.none { it.hash == gitSelectedCommit?.hash }) {
            gitBackToCommits()
        }
    }

    fun gitSelectCommit(commit: GitCommitInfo) {
        gitSelectedCommit = commit
        gitSinceMode = false
        viewModelScope.launch { loadGitFiles() }
    }

    fun gitSetSinceMode(since: Boolean) {
        gitSinceMode = since
        viewModelScope.launch { loadGitFiles() }
    }

    private suspend fun loadGitFiles() {
        val g = git ?: return
        val cat = catalog ?: return
        val commit = gitSelectedCommit ?: return
        val changes = runCatching {
            if (gitSinceMode) g.changesSince(commit.hash) else g.commitFiles(commit.hash)
        }.getOrElse {
            gitError = it.message
            return
        }
        gitFiles = changes
            .filterNot { it.path.substringAfterLast('/').startsWith(".") }
            .map { change ->
                // путь в репозитории -> идентификатор файла каталога
                val id =
                    if (change.path == NeurodocCatalog.ROOT_ID) NeurodocCatalog.ROOT_ID
                    else NeurodocCatalog.tryResolveLink(NeurodocCatalog.ROOT_ID, change.path)
                val deleted = change.status == 'D'
                val title = if (!deleted && id != null && cat.exists(id)) {
                    cat.title(id)
                } else {
                    // файла уже нет — заголовок берём из прежней ревизии
                    val text = g.readBlob(
                        commit.hash, change.oldPath ?: change.path,
                        fromParent = !gitSinceMode,
                    )
                    firstNonEmptyLine(text) ?: change.path
                }
                val statusText = when (change.status) {
                    'A' -> "добавлен"
                    'D' -> "удалён"
                    'R' -> "переименован"
                    else -> "изменён"
                }
                GitFileRow(
                    path = change.path,
                    fileId = id,
                    title = title,
                    detail = "$statusText  ${change.path}",
                    deleted = deleted,
                )
            }
    }

    private fun firstNonEmptyLine(text: String?): String? {
        if (text == null) return null
        for (raw in text.lineSequence()) {
            val line = raw.trim().trimStart('#').trim()
            if (line.isNotEmpty()) return line.take(60)
        }
        return null
    }

    /** Тап по файлу — открыть его в редакторе; дельту показывает отдельная кнопка. */
    fun gitOpenFile(row: GitFileRow) {
        val cat = catalog ?: return
        val id = row.fileId
        if (row.deleted || id == null || !cat.exists(id)) {
            statusMessage = "Файла больше нет в каталоге"
            return
        }
        openFile(id)
    }

    fun gitShowDiff(row: GitFileRow) {
        val g = git ?: return
        val commit = gitSelectedCommit ?: return
        viewModelScope.launch {
            runCatching {
                val diff = g.fileDiff(commit.hash, row.path, gitSinceMode)
                gitDiff = row.title to diff
            }.onFailure { gitError = it.message }
        }
    }

    override fun onCleared() {
        saveNow()
        super.onCleared()
    }
}

/** Какой конфликт показать пользователю: PUSH и PULL — по одной кнопке выбора, SYNC — обе. */
enum class GitConflictKind { PUSH, PULL, SYNC }

/** Строка списка изменённых файлов в панели git-истории. */
data class GitFileRow(
    val path: String,
    val fileId: String?,
    val title: String,
    val detail: String,
    val deleted: Boolean,
)
