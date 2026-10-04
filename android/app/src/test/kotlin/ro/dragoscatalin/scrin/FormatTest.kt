package ro.dragoscatalin.scrin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ro.dragoscatalin.scrin.data.RelayUrls
import ro.dragoscatalin.scrin.data.ServerUrl
import ro.dragoscatalin.scrin.ui.ConnectForm
import ro.dragoscatalin.scrin.ui.Format

class FormatTest {
    @Test fun countdownFormatsMinutesAndClampsNegative() {
        assertEquals("10:00", Format.countdown(600))
        assertEquals("0:09", Format.countdown(9))
        assertEquals("0:00", Format.countdown(-5))
    }

    @Test fun durationSwitchesToHours() {
        assertEquals("0:42", Format.duration(42_000))
        assertEquals("59:59", Format.duration(3_599_000))
        assertEquals("1:00:01", Format.duration(3_601_000))
    }

    @Test fun codeInputUppercasesFiltersAndInsertsDash() {
        assertEquals("ABCD-EFGH", Format.codeInput("abcdefgh"))
        assertEquals("ABCD-EFGH", Format.codeInput("ab cd-ef gh zz"))
        // I, L, O, U, 0, 1 are not in the alphabet.
        assertEquals("ABC", Format.codeInput("aIbLcO01"))
        assertEquals("ABCD", Format.codeInput("abcd"))
        assertTrue(Format.isCompleteCode(Format.codeInput("abcdefgh")))
        assertFalse(Format.isCompleteCode("ABCD-EFG"))
    }

    @Test fun codeCaretFollowsTypingAcrossTheDash() {
        // Typing the 5th symbol at the end of "ABCD" must leave the caret after it, past the dash,
        // so the next symbol is appended (live bug: "M8XJVT3H" became "M8XJ-T3HV").
        assertEquals(6, Format.codeCaret("ABCDE", 5))
        assertEquals(7, Format.codeCaret("ABCD-EF", 7))
        assertEquals(4, Format.codeCaret("ABCD", 4))
        assertEquals(2, Format.codeCaret("ABCD-EF", 2))
        assertEquals(9, Format.codeCaret("ABCD-EFGHJ", 10))
    }

    @Test fun shortTicketKeepsHeadAndTail() {
        val t = "scrin1" + "a".repeat(80) + "xyz123"
        val s = Format.shortTicket(t)
        assertTrue(s.startsWith("scrin1aaaaaa"))
        assertTrue(s.endsWith("xyz123"))
        assertEquals("short", Format.shortTicket("short"))
    }

    @Test fun progressIsClamped() {
        assertEquals(0.5f, Format.progress(300, 600), 0.0001f)
        assertEquals(0f, Format.progress(-1, 600), 0.0001f)
        assertEquals(1f, Format.progress(900, 600), 0.0001f)
        assertEquals(0f, Format.progress(10, 0), 0.0001f)
    }

    @Test fun connectFormValidation() {
        val hex = "ab".repeat(32)
        val ticket = "scrin1" + "a".repeat(70)
        assertEquals(ConnectForm.Problem.TARGET_EMPTY, ConnectForm.validate("  ", "ABCD-EFGH"))
        assertEquals(ConnectForm.Problem.TARGET_INVALID, ConnectForm.validate("hello", "ABCD-EFGH"))
        assertEquals(ConnectForm.Problem.CODE_INCOMPLETE, ConnectForm.validate(hex, "ABCD"))
        assertNull(ConnectForm.validate(hex, "ABCD-EFGH"))
        assertNull(ConnectForm.validate(" $ticket \n", "ABCD-EFGH"))
        assertNull(ConnectForm.validate("scrin:$hex?a=192.168.1.5:7000&r=https://relay.example.org/", "ABCD-EFGH"))
        assertNull(ConnectForm.validate("scrin:$hex", "ABCD-EFGH"))
        assertEquals(ConnectForm.Problem.TARGET_INVALID, ConnectForm.validate("scrin:abcd", "ABCD-EFGH"))
        assertNull(ConnectForm.validate("123 456-789", "ABCD-EFGH"))
        assertEquals("123456789", ConnectForm.normalizeTarget(" 123-456 789 "))
        assertEquals(ConnectForm.Problem.TARGET_INVALID, ConnectForm.validate("12345678", "ABCD-EFGH"))
    }

    @Test fun serverUrlAllowsHttpOnlyOnPrivateNetworks() {
        assertTrue(ServerUrl.valid(""))
        assertEquals("https://scrin.example.org", ServerUrl.parse(" https://scrin.example.org "))
        assertEquals("http://192.168.100.61:8443", ServerUrl.parse("http://192.168.100.61:8443"))
        assertEquals("http://100.95.246.105:8443", ServerUrl.parse("http://100.95.246.105:8443"))
        assertNull(ServerUrl.parse("http://scrin.example.org"))
        assertNull(ServerUrl.parse("ftp://x"))
        assertNull(ServerUrl.parse(""))
    }

    @Test fun relayUrlsParse() {
        assertEquals(emptyList<String>(), RelayUrls.parse(""))
        assertEquals(listOf("https://a.example", "https://b.example"), RelayUrls.parse(" https://a.example , https://b.example "))
        assertNull(RelayUrls.parse("http://insecure.example"))
        assertNull(RelayUrls.parse("https://"))
    }
}
