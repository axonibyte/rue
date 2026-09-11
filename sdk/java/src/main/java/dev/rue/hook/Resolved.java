package dev.rue.hook;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * A value of an {@code execute.run} body after resolution: its text, and
 * whether it is a secret.
 *
 * <p>7.11 has an SDK expose a run's secrets to the handler "without ever
 * placing them on a command line". The text is reachable -- through
 * {@link Hooks#expose}, named so that reading a secret is a visible act --
 * but formatting the value, which is how a secret reaches a command line or
 * a log by accident, gives {@value #REDACTED} and never the text. The Rust
 * and Python SDKs carry the same type.
 */
public record Resolved(String text, boolean secret) {

    /** What a secret reads as wherever it is formatted. */
    public static final String REDACTED = "<secret>";

    /** The text if the value is not a secret, and {@value #REDACTED} if it is. */
    @Override
    public String toString() {
        return secret ? REDACTED : text;
    }

    /** A resolved value as the wire carries it: {@code {"text": .., "secret": ..}}. */
    static boolean isWire(Object v) {
        return v instanceof Map<?, ?> m && m.containsKey("text");
    }

    static Resolved of(Map<?, ?> wire) {
        Object text = wire.get("text");
        return new Resolved(text == null ? "" : String.valueOf(text), Boolean.TRUE.equals(wire.get("secret")));
    }

    /**
     * A body as the handler receives it: the same maps and lists as the
     * wire, with every resolved value -- a primitive's field, or the value
     * of an {@code env} pair -- a {@link Resolved}.
     */
    @SuppressWarnings("unchecked")
    static List<Object> body(List<Object> wire) {
        List<Object> out = new ArrayList<>(wire.size());
        for (Object prim : wire) {
            if (!(prim instanceof Map<?, ?> p)) {
                out.add(prim);
                continue;
            }
            Map<String, Object> converted = new LinkedHashMap<>();
            for (Map.Entry<?, ?> e : p.entrySet()) {
                Object fields = e.getValue();
                if (fields instanceof Map<?, ?> f) {
                    Map<String, Object> m = new LinkedHashMap<>();
                    for (Map.Entry<?, ?> fe : f.entrySet()) {
                        m.put(String.valueOf(fe.getKey()), field(String.valueOf(fe.getKey()), fe.getValue()));
                    }
                    converted.put(String.valueOf(e.getKey()), m);
                } else {
                    converted.put(String.valueOf(e.getKey()), fields);
                }
            }
            out.add(converted);
        }
        return out;
    }

    private static Object field(String name, Object v) {
        if (isWire(v)) {
            return of((Map<?, ?>) v);
        }
        if (name.equals("env") && v instanceof List<?> pairs) {
            List<Object> out = new ArrayList<>(pairs.size());
            for (Object pair : pairs) {
                if (pair instanceof List<?> kv && kv.size() == 2 && isWire(kv.get(1))) {
                    List<Object> p = new ArrayList<>(2);
                    p.add(kv.get(0));
                    p.add(of((Map<?, ?>) kv.get(1)));
                    out.add(p);
                } else {
                    out.add(pair);
                }
            }
            return out;
        }
        return v;
    }
}
