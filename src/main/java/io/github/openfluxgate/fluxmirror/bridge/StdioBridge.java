package io.github.openfluxgate.fluxmirror.bridge;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

public class StdioBridge {

    private static final Logger log = LoggerFactory.getLogger(StdioBridge.class);
    private static final int BUFFER_SIZE = 8192;

    private final InputStream parentIn;
    private final OutputStream parentOut;
    private final ChildProcess child;

    public StdioBridge(InputStream parentIn, OutputStream parentOut, ChildProcess child) {
        this.parentIn = parentIn;
        this.parentOut = parentOut;
        this.child = child;
    }

    public void run() throws InterruptedException {
        Thread c2s = Thread.ofVirtual().name("c2s").start(() -> relay(parentIn, child.stdin(), "c2s"));
        Thread s2c = Thread.ofVirtual().name("s2c").start(() -> relay(child.stdout(), parentOut, "s2c"));

        log.info("relay started");

        c2s.join();
        s2c.join();
    }

    private void relay(InputStream in, OutputStream out, String direction) {
        byte[] buf = new byte[BUFFER_SIZE];
        try {
            int n;
            while ((n = in.read(buf)) != -1) {
                out.write(buf, 0, n);
                out.flush();
            }
        } catch (IOException e) {
            log.debug("relay {} IOException: {}", direction, e.getMessage());
        }
        log.info("relay stopped direction={}", direction);
    }
}
