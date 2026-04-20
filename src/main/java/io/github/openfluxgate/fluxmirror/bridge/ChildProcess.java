package io.github.openfluxgate.fluxmirror.bridge;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.util.List;
import java.util.concurrent.TimeUnit;

public class ChildProcess implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(ChildProcess.class);

    private final List<String> command;
    private Process process;

    public ChildProcess(List<String> command) {
        this.command = List.copyOf(command);
    }

    public void start() throws IOException {
        log.debug("spawning child process: {}", command);
        ProcessBuilder pb = new ProcessBuilder(command);
        pb.redirectError(ProcessBuilder.Redirect.INHERIT);
        process = pb.start();
        log.debug("child process started, pid={}", process.pid());
    }

    public long pid() {
        return process.pid();
    }

    @Override
    public void close() {
        if (process == null || !process.isAlive()) {
            log.info("child process already stopped");
            return;
        }

        log.info("sending SIGTERM to child pid={}", process.pid());
        process.destroy();

        try {
            if (process.waitFor(2, TimeUnit.SECONDS)) {
                log.info("child exited after SIGTERM, exit code={}", process.exitValue());
                return;
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }

        log.info("child did not exit in time, sending SIGKILL to pid={}", process.pid());
        process.destroyForcibly();

        try {
            process.waitFor(1, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        log.info("child forcibly terminated");
    }
}
