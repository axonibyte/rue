package dev.rue.hook;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

/**
 * The codec on inputs no conformance case sends. Conformance proves the
 * messages the engine does send round-trip; these are the ones it might
 * one day send, and the malformed ones a pipe can deliver.
 */
class JsonTest {

    @Test
    void anObjectRoundTripsWithItsOrderAndItsIntegersIntact() {
        String text = "{\"id\":7,\"kind\":\"execute\",\"list\":[1,2.5,true,null,\"x\"],\"nested\":{\"a\":{}}}";
        Map<String, Object> m = Json.parseObject(text);
        assertEquals(List.of("id", "kind", "list", "nested"), List.copyOf(m.keySet()));
        // An id stays an integer: 7, not 7.0.
        assertInstanceOf(Long.class, m.get("id"));
        assertEquals(text, Json.write(m));
    }

    @Test
    void everyEscapeIsReadAndControlCharactersAreWrittenEscaped() {
        assertEquals("q\" b\\ s/ \b\f\n\r\t", Json.parse("\"q\\\" b\\\\ s\\/ \\b\\f\\n\\r\\t\""));
        String written = Json.write("a\u0001b\nc");
        assertEquals("\"a\\u0001b\\nc\"", written);
        assertEquals("a\u0001b\nc", Json.parse(written));
    }

    @Test
    void textOutsideAsciiSurvivesRawAndAsASurrogatePairEscape() {
        assertEquals("é中\uD83D\uDE00", Json.parse("\"é中\\ud83d\\ude00\""));
        assertEquals("é中\uD83D\uDE00", Json.parse(Json.write("é中\uD83D\uDE00")));
    }

    @Test
    void numbersKeepTheirKind() {
        assertEquals(-1L, Json.parse("-1"));
        assertEquals(1500.0, Json.parse("1.5e3"));
        assertEquals(0.25, Json.parse("0.25"));
        assertEquals("1700000000", Json.write(1700000000L));
        // An integral double leaves as an integer when it fits one...
        assertEquals("3", Json.write(3.0));
    }

    @Test
    void anIntegralDoubleBeyondALongIsWrittenAsItselfNotClamped() {
        // ...and as itself when it does not: casting 1e20 to a long gives
        // Long.MAX_VALUE, a different number written without complaint.
        Object back = Json.parse(Json.write(1e20));
        assertEquals(1e20, ((Number) back).doubleValue());
    }

    @Test
    void aNumberJsonCannotSpellIsRefusedRatherThanWrittenAsNaN() {
        assertThrows(IllegalArgumentException.class, () -> Json.write(Double.NaN));
        assertThrows(IllegalArgumentException.class, () -> Json.write(Double.POSITIVE_INFINITY));
    }

    @Test
    void aTruncatedUnicodeEscapeIsAParseErrorNotAnIndexError() {
        // The serve loop skips a line the codec calls malformed; an index
        // error is not that call, and any caller catching the codec's own
        // exception would not catch it.
        assertThrows(IllegalArgumentException.class, () -> Json.parse("\"\\u12\""));
        assertThrows(IllegalArgumentException.class, () -> Json.parse("\"\\u12zz\""));
    }

    @Test
    void deepNestingIsRefusedRatherThanOverflowingTheStack() {
        // A StackOverflowError is an Error, which nothing in the serve loop
        // catches: one line of brackets would end the hook.
        String deep = "[".repeat(100_000) + "]".repeat(100_000);
        assertThrows(IllegalArgumentException.class, () -> Json.parse(deep));
    }

    @Test
    void malformedTextIsRefused() {
        for (String bad : List.of("", "   ", "{", "{\"a\":}", "[1,]", "tru", "nul", "{\"a\" 1}", "[1] 2", "\"open")) {
            assertThrows(IllegalArgumentException.class, () -> Json.parse(bad), bad);
        }
        assertThrows(IllegalArgumentException.class, () -> Json.parseObject("[1]"));
    }

    @Test
    void aDuplicateKeyKeepsTheLastValue() {
        Map<String, Object> m = Json.parseObject("{\"a\":1,\"a\":2}");
        assertEquals(2L, m.get("a"));
        assertEquals(1, m.size());
    }

    @Test
    void nullKeysAndValuesAreWrittenAsJson() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("gone", null);
        assertEquals("{\"gone\":null}", Json.write(m));
        assertTrue(Json.parseObject("{\"gone\":null}").containsKey("gone"));
        assertFalse(Json.parseObject("{}").containsKey("gone"));
    }
}
