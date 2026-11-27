package cn.juniexd.nexttodo.widget

import android.content.Context
import cn.juniexd.nexttodo.Due
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.temporal.ChronoUnit

/** What the launcher's list of widgets shows them with (docs/android/widgets.md): example tasks, in the
 * look the user chose. Not the user's own, which the system keeps showing there after they sign out. */
object Sample {
    private class Task(val title: String, val folder: String, val inDays: Long, val time: LocalTime?)

    private val TASKS = listOf(
        Task("写周报", "工作", 0, LocalTime.of(18, 0)),
        Task("交电费", "生活", 0, null),
        Task("组会汇报", "学习 / 课题组", 1, LocalTime.of(14, 0)),
        Task("预约体检", "生活", 2, null),
        Task("取快递", "生活", 4, LocalTime.of(19, 30)),
    )

    private fun now() = LocalDateTime.now().truncatedTo(ChronoUnit.MINUTES)

    private fun entries(today: LocalDate) =
        TASKS.mapIndexed { i, t -> Entry("sample-$i", t.title, t.folder, Due(today.plusDays(t.inDays), t.time), later = false, ready = true, occurrence = null) }

    fun shown(context: Context): Shown {
        val now = now()
        val items = entries(now.toLocalDate()).map { Item(it.id, it.title, it.folder, it.due, null, null, null, null) }
        return Shown(true, items, 0, emptySet(), Widget.look(context), now)
    }

    fun agendas(context: Context): Agendas {
        val now = now()
        return Agendas(true, entries(now.toLocalDate()).groupBy { it.due.date }.map { (date, entries) -> Day(date, entries) }, 0, Widget.look(context), now)
    }

    /** The week as the user's look has it, with what is written with its days. */
    fun week(context: Context): Week {
        val week = Agenda.week(context)
        return week.copy(signedIn = true, due = entries(week.now.toLocalDate()).groupBy { it.due.date }, overdue = emptyList(), ticked = emptyList())
    }
}
