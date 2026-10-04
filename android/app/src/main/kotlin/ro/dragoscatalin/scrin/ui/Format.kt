package ro.dragoscatalin.scrin.ui

import java.util.Locale

/** Pure formatting helpers shared by screens (unit-tested on the JVM). */
object Format {
    /** Same alphabet as scrin-crypto `code::ALPHABET` (no I, L, O, U, 0, 1). */
    const val CODE_ALPHABET = "ABCDEFGHJKMNPQRSTVWXYZ23456789"
    const val CODE_LEN = 8

    /** `m:ss` for a countdown; negative clamps to 0. */
    fun countdown(seconds: Long): String {
        val s = seconds.coerceAtLeast(0)
        return String.format(Locale.ROOT, "%d:%02d", s / 60, s % 60)
    }

    /** `h:mm:ss` or `m:ss` for a session duration. */
    fun duration(ms: Long): String {
        val total = (ms / 1000).coerceAtLeast(0)
        val h = total / 3600
        val m = (total % 3600) / 60
        val s = total % 60
        return if (h > 0) String.format(Locale.ROOT, "%d:%02d:%02d", h, m, s) else String.format(Locale.ROOT, "%d:%02d", m, s)
    }

    /**
     * Normalises what the user types into a code field: uppercase, drops anything
     * outside the alphabet, inserts the dash after 4 symbols, caps at 8 symbols.
     */
    fun codeInput(raw: String): String {
        val symbols = raw.uppercase(Locale.ROOT).filter { it in CODE_ALPHABET }.take(CODE_LEN)
        return if (symbols.length > 4) symbols.substring(0, 4) + "-" + symbols.substring(4) else symbols
    }

    fun isCompleteCode(display: String): Boolean = display.count { it in CODE_ALPHABET } == CODE_LEN && display.length == CODE_LEN + 1

    /**
     * Caret position in [codeInput]`(raw)` matching caret [rawCaret] in [raw]: the same number of
     * code symbols sit before it, and it lands after the dash once the fifth symbol is typed.
     */
    fun codeCaret(raw: String, rawCaret: Int): Int {
        val before = raw.take(rawCaret.coerceIn(0, raw.length)).uppercase(Locale.ROOT).count { it in CODE_ALPHABET }.coerceAtMost(CODE_LEN)
        val formatted = codeInput(raw)
        return (if (before > 4) before + 1 else before).coerceAtMost(formatted.length)
    }

    /** `scrin:abcdef…uvwxyz` for long tickets; short strings are returned as-is. */
    fun shortTicket(ticket: String, head: Int = 12, tail: Int = 6): String =
        if (ticket.length <= head + tail + 1) ticket else ticket.take(head) + "…" + ticket.takeLast(tail)

    /** 0..1 fraction of the code's lifetime left. */
    fun progress(remainingMs: Long, totalMs: Long): Float =
        if (totalMs <= 0) 0f else (remainingMs.toFloat() / totalMs).coerceIn(0f, 1f)
}

/** Validation of the "connect to a device" form, without touching the native core. */
object ConnectForm {
    enum class Problem { TARGET_EMPTY, TARGET_INVALID, CODE_INCOMPLETE }

    private val HEX_ID = Regex("^[0-9a-fA-F]{64}$")
    /** Compact legacy form. */
    private val TICKET = Regex("^scrin1[a-z2-7]{60,}$")
    /** Desktop-engine form `scrin:<64 hex>[?a=ip:port&r=url…]`. */
    private val TICKET_URI = Regex("^scrin:[0-9a-fA-F]{64}(\\?\\S*)?$")
    private val SCRIN_ID = Regex("^\\d{9}$")

    /** Whitespace dropped; a scrin ID typed as `123 456 789` / `123-456-789` becomes 9 digits. */
    fun normalizeTarget(raw: String): String {
        val t = raw.filterNot { it.isWhitespace() }
        val digits = t.replace("-", "")
        return if (SCRIN_ID.matches(digits)) digits else t
    }

    fun validate(target: String, code: String): Problem? {
        val t = normalizeTarget(target)
        return when {
            t.isEmpty() -> Problem.TARGET_EMPTY
            !(HEX_ID.matches(t) || TICKET.matches(t) || TICKET_URI.matches(t) || SCRIN_ID.matches(t)) -> Problem.TARGET_INVALID
            !Format.isCompleteCode(code) -> Problem.CODE_INCOMPLETE
            else -> null
        }
    }
}
