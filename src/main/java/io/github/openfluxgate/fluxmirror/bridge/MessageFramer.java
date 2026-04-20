package io.github.openfluxgate.fluxmirror.bridge;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.List;

public class MessageFramer {

    private static final Logger log = LoggerFactory.getLogger(MessageFramer.class);
    private static final int MAX_BUFFER_SIZE = 10 * 1024 * 1024; // 10 MB

    private final ByteArrayOutputStream buffer = new ByteArrayOutputStream(4096);
    private boolean discarding;

    public List<byte[]> feed(byte[] buf, int offset, int length) {
        List<byte[]> messages = new ArrayList<>();

        for (int i = offset; i < offset + length; i++) {
            byte b = buf[i];

            if (!discarding && buffer.size() > MAX_BUFFER_SIZE) {
                discarding = true;
                log.warn("message exceeded {} bytes, discarding until next newline", MAX_BUFFER_SIZE);
                buffer.reset();
            }

            if (discarding) {
                if (b == 0x0A) {
                    discarding = false;
                }
                continue;
            }

            if (b == 0x0A) {
                byte[] raw = buffer.toByteArray();
                // Strip trailing \r if present
                if (raw.length > 0 && raw[raw.length - 1] == 0x0D) {
                    byte[] trimmed = new byte[raw.length - 1];
                    System.arraycopy(raw, 0, trimmed, 0, trimmed.length);
                    messages.add(trimmed);
                } else {
                    messages.add(raw);
                }
                buffer.reset();
            } else {
                buffer.write(b);
            }
        }

        return messages;
    }

    public void reset() {
        buffer.reset();
        discarding = false;
    }
}
