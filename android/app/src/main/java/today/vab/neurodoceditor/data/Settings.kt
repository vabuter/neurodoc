package today.vab.neurodoceditor.data

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

data class ProjectInfo(
    val id: String = UUID.randomUUID().toString(),
    val name: String = "",
    val path: String = "",
    /**
     * Фон области просмотра SVG: "Light", "Dark" или null — «как тема приложения».
     *
     * Настройка проекта, а не приложения: рисунки одного проекта нарисованы под один
     * фон. Пока пользователь не переключал фон в самом просмотре, тут null и работает
     * тема; первое же переключение запоминается и дальше побеждает тему.
     */
    val svgBackground: String? = null,
)

/** Тема оформления: как в Windows-версии — Система / Светлая / Тёмная. */
enum class ThemeMode { SYSTEM, LIGHT, DARK }

/**
 * Доступ к git-серверу: токен по имени хоста (github.com, gitlab.com). Один токен
 * обслуживает все проекты с этого хоста. Хранится в settings.json в приватной папке
 * приложения.
 */
data class GitAccount(
    val host: String = "",
    /** Имя для Basic-авторизации; пустое — подставится подходящее хосту. */
    val username: String = "",
    val token: String = "",
)

data class AppSettings(
    val theme: ThemeMode = ThemeMode.SYSTEM,
    val fontFamily: String = FONT_JETBRAINS,
    val fontSize: Float = 15f,
    val lineSpacing: Float = 1.2f,
    /**
     * Режим «как есть»: файл показывается ровно как на диске, служебные части ссылок
     * не скрываются. Нужен, чтобы править текст посимвольно.
     */
    val rawEditing: Boolean = false,
    val currentProjectId: String? = null,
    val projects: List<ProjectInfo> = emptyList(),
    /** Автор git-коммитов; git config на Android нет, identity живёт здесь. */
    val gitUserName: String = "",
    val gitUserEmail: String = "",
    val gitAccounts: List<GitAccount> = emptyList(),
) {
    val currentProject: ProjectInfo? get() = projects.firstOrNull { it.id == currentProjectId }

    companion object {
        const val FONT_JETBRAINS = "JetBrains Mono"
        const val FONT_INTER = "Inter"
        const val FONT_SYSTEM = "Системный"
        val FONTS = listOf(FONT_JETBRAINS, FONT_INTER, FONT_SYSTEM)
    }
}

/** Хранилище настроек: JSON в приватной папке приложения (аналог settings.yaml в Windows). */
class SettingsStore(context: Context) {

    private val file = File(context.filesDir, "settings.json")

    fun load(): AppSettings {
        if (!file.exists()) return AppSettings()
        return runCatching {
            val json = JSONObject(file.readText())
            val projects = mutableListOf<ProjectInfo>()
            val array = json.optJSONArray("projects") ?: JSONArray()
            for (i in 0 until array.length()) {
                val item = array.getJSONObject(i)
                val path = item.optString("path")
                if (path.isNotBlank()) {
                    projects.add(
                        ProjectInfo(
                            id = item.optString("id").ifBlank { UUID.randomUUID().toString() },
                            name = item.optString("name"),
                            path = path,
                            svgBackground = item.optString("svgBackground").takeIf { it.isNotBlank() },
                        )
                    )
                }
            }
            val accounts = mutableListOf<GitAccount>()
            val accountsArray = json.optJSONArray("gitAccounts") ?: JSONArray()
            for (i in 0 until accountsArray.length()) {
                val item = accountsArray.getJSONObject(i)
                val host = item.optString("host")
                if (host.isNotBlank()) {
                    accounts.add(
                        GitAccount(
                            host = host,
                            username = item.optString("username"),
                            token = item.optString("token"),
                        )
                    )
                }
            }
            val currentId = json.optString("currentProjectId").takeIf { it.isNotBlank() }
            AppSettings(
                theme = runCatching { ThemeMode.valueOf(json.optString("theme", "SYSTEM")) }
                    .getOrDefault(ThemeMode.SYSTEM),
                fontFamily = json.optString("fontFamily", AppSettings.FONT_JETBRAINS),
                fontSize = json.optDouble("fontSize", 15.0).toFloat().coerceIn(8f, 40f),
                lineSpacing = json.optDouble("lineSpacing", 1.2).toFloat().coerceIn(0.8f, 3f),
                rawEditing = json.optBoolean("rawEditing", false),
                currentProjectId = currentId?.takeIf { id -> projects.any { it.id == id } }
                    ?: projects.firstOrNull()?.id,
                projects = projects,
                gitUserName = json.optString("gitUserName"),
                gitUserEmail = json.optString("gitUserEmail"),
                gitAccounts = accounts,
            )
        }.getOrDefault(AppSettings())
    }

    fun save(settings: AppSettings) {
        runCatching {
            val array = JSONArray()
            settings.projects.forEach { project ->
                array.put(
                    JSONObject()
                        .put("id", project.id)
                        .put("name", project.name)
                        .put("path", project.path)
                        .put("svgBackground", project.svgBackground ?: "")
                )
            }
            val accountsArray = JSONArray()
            settings.gitAccounts.forEach { account ->
                accountsArray.put(
                    JSONObject()
                        .put("host", account.host)
                        .put("username", account.username)
                        .put("token", account.token)
                )
            }
            val json = JSONObject()
                .put("theme", settings.theme.name)
                .put("fontFamily", settings.fontFamily)
                .put("fontSize", settings.fontSize.toDouble())
                .put("lineSpacing", settings.lineSpacing.toDouble())
                .put("rawEditing", settings.rawEditing)
                .put("currentProjectId", settings.currentProjectId ?: "")
                .put("projects", array)
                .put("gitUserName", settings.gitUserName)
                .put("gitUserEmail", settings.gitUserEmail)
                .put("gitAccounts", accountsArray)
            file.writeText(json.toString(2))
        }
    }
}
