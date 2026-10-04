package ro.dragoscatalin.scrin.service

/**
 * Printable USB HID keyboard usages (US layout) → the character to type on an Android host.
 * A desktop controller sends physical keys, not text; Android has no key injection for a
 * non-system app, so printable keys become `ACTION_SET_TEXT` edits.
 */
object HidText {
    private const val MOD_SHIFT = 1u
    /** Ctrl (2), Alt (4), Meta (8): a shortcut, not text. */
    private const val MOD_CHORD = 14u

    private const val DIGITS = "1234567890"
    private const val DIGITS_SHIFTED = "!@#$%^&*()"

    /** Usage → (plain, shifted) for the punctuation keys. */
    private val PUNCT = mapOf(
        0x2Cu to (' ' to ' '),
        0x2Du to ('-' to '_'),
        0x2Eu to ('=' to '+'),
        0x2Fu to ('[' to '{'),
        0x30u to (']' to '}'),
        0x31u to ('\\' to '|'),
        0x33u to (';' to ':'),
        0x34u to ('\'' to '"'),
        0x35u to ('`' to '~'),
        0x36u to (',' to '<'),
        0x37u to ('.' to '>'),
        0x38u to ('/' to '?'),
    )

    fun char(usage: UInt, modifiers: UInt): Char? {
        if (modifiers and MOD_CHORD != 0u) return null
        val shift = modifiers and MOD_SHIFT != 0u
        return when (usage) {
            in 0x04u..0x1Du -> ('a' + (usage - 0x04u).toInt()).let { if (shift) it.uppercaseChar() else it }
            in 0x1Eu..0x27u -> (if (shift) DIGITS_SHIFTED else DIGITS)[(usage - 0x1Eu).toInt()]
            else -> PUNCT[usage]?.let { (plain, shifted) -> if (shift) shifted else plain }
        }
    }
}
