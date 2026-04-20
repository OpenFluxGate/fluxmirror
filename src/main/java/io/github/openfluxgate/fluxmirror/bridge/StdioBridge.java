package io.github.openfluxgate.fluxmirror.bridge;

import io.github.openfluxgate.fluxmirror.model.Event;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.BlockingQueue;

public class StdioBridge {

    private static final Logger log = LoggerFactory.getLogger(StdioBridge.class);
    private static final int BUFFER_SIZE = 8192;
    private static final int LOG_TRUNCATE_CHARS = 2000;

    private final InputStream parentIn;
    private final OutputStream parentOut;
    private final ChildProcess child;
    private final OutputStream captureC2s;
    private final OutputStream captureS2c;
    private final BlockingQueue<Event> eventQueue;
    private final String serverName;

    public StdioBridge(InputStream parentIn, OutputStream parentOut, ChildProcess child,
                       OutputStream captureC2s, OutputStream captureS2c,
                       BlockingQueue<Event> eventQueue, String serverName) {
        this.parentIn = parentIn;
        this.parentOut = parentOut;
        this.child = child;
        this.captureC2s = captureC2s;
        this.captureS2c = captureS2c;
        this.eventQueue = eventQueue;
        this.serverName = serverName;
    }

    public void run() throws InterruptedException {
        Thread c2s = Thread.ofVirtual().name("c2s").start(() -> relay(parentIn, child.stdin(), "c2s", captureC2s));
        Thread s2c = Thread.ofVirtual().name("s2c").start(() -> relay(child.stdout(), parentOut, "s2c", captureS2c));

        log.info("relay started");

        c2s.join();
        s2c.join();
    }

    private void relay(InputStream in, OutputStream out, String direction, OutputStream capture) {
        boolean captureFailed = false;
        boolean framerFailed = false;
        boolean queueFull = false;
        MessageFramer framer = new MessageFramer();
        byte[] buf = new byte[BUFFER_SIZE];
        try {
            int n;
            while ((n = in.read(buf)) != -1) {
                // 1. Relay: absolute priority
                out.write(buf, 0, n);
                out.flush();

                // 2. Capture: best-effort
                if (capture != null && !captureFailed) {
                    try {
                        capture.write(buf, 0, n);
                        capture.flush();
                    } catch (IOException e) {
                        captureFailed = true;
                        log.warn("capture {} write failed, disabling: {}", direction, e.getMessage());
                    }
                }

                // 3. Framer + logging + event queue: best-effort
                if (!framerFailed) {
                    try {
                        List<byte[]> messages = framer.feed(buf, 0, n);
                        for (byte[] msg : messages) {
                            String text = new String(msg, StandardCharsets.UTF_8);
                            if (text.length() > LOG_TRUNCATE_CHARS) {
                                text = text.substring(0, LOG_TRUNCATE_CHARS)
                                        + "... (" + msg.length + " bytes total)";
                            }
                            log.info("[{}] {}", direction, text);

                            Event event = new Event(System.currentTimeMillis(), direction, serverName, msg);
                            if (!eventQueue.offer(event) && !queueFull) {
                                queueFull = true;
                                log.warn("event queue full, dropping events for direction={}", direction);
                            }
                        }
                    } catch (Exception e) {
                        framerFailed = true;
                        log.warn("framer {} failed, disabling: {}", direction, e.getMessage());
                    }
                }
            }
        } catch (IOException e) {
            log.debug("relay {} IOException: {}", direction, e.getMessage());
        }
        log.info("relay stopped direction={}", direction);
    }
}
