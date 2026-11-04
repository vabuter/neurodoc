package today.vab.neurodoceditor.data

import android.content.Context
import org.json.JSONObject
import java.io.File

/**
 * Что читатель уже прошёл в этом проекте. Состояние читателя, а не документации,
 * поэтому лежит рядом с настройками, а не в каталоге neurodoc.
 */
class ReadingProgress(private val visited: MutableMap<String, Long> = HashMap()) {

    fun isRead(id: String): Boolean = visited.containsKey(id.lowercase())

    fun mark(id: String, read: Boolean) {
        if (read) visited[id.lowercase()] = System.currentTimeMillis() else visited.remove(id.lowercase())
    }

    fun toJson(): JSONObject {
        val json = JSONObject()
        visited.forEach { (id, at) -> json.put(id, at) }
        return JSONObject().put("visited", json)
    }

    companion object {
        fun fromJson(json: JSONObject): ReadingProgress {
            val visited = HashMap<String, Long>()
            val node = json.optJSONObject("visited") ?: JSONObject()
            for (key in node.keys()) visited[key.lowercase()] = node.optLong(key)
            return ReadingProgress(visited)
        }
    }
}

/**
 * Прогресс чтения: отдельный файл на проект в приватной папке приложения.
 *
 * Отдельно от settings.json намеренно: список прочитанного растёт и переписывается
 * при каждом переходе, настройкам такое соседство ни к чему. Сброс прогресса —
 * удаление файла.
 */
class ReadingProgressStore(context: Context) {

    private val dir = File(context.filesDir, "reading")

    fun fileFor(projectId: String): File =
        File(dir, projectId.replace(Regex("""[^\w.-]"""), "_") + ".json")

    /** Прогресс с диска; повреждённый файл не мешает читать — прогресс просто пуст. */
    fun load(projectId: String): ReadingProgress = runCatching {
        val file = fileFor(projectId)
        if (!file.exists()) ReadingProgress() else ReadingProgress.fromJson(JSONObject(file.readText()))
    }.getOrDefault(ReadingProgress())

    /** Сбой записи проглатываем: прогресс — не та потеря, ради которой стоит прерывать работу. */
    fun save(projectId: String, progress: ReadingProgress) {
        runCatching {
            dir.mkdirs()
            fileFor(projectId).writeText(progress.toJson().toString())
        }
    }

    fun reset(projectId: String) {
        runCatching { fileFor(projectId).delete() }
    }
}
