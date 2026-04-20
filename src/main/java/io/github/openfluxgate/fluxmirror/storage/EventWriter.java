package io.github.openfluxgate.fluxmirror.storage;

import io.github.openfluxgate.fluxmirror.model.Event;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.TimeUnit;

public class EventWriter {

    private static final Logger log = LoggerFactory.getLogger(EventWriter.class);

    private final EventStore store;
    private final BlockingQueue<Event> queue;
    private Thread thread;

    public EventWriter(EventStore store, BlockingQueue<Event> queue) {
        this.store = store;
        this.queue = queue;
    }

    public Thread start() {
        thread = new Thread(this::run, "event-writer");
        thread.start();
        return thread;
    }

    public void stop() {
        if (thread != null) {
            thread.interrupt();
        }
    }

    private void run() {
        log.info("event writer started");

        try {
            while (!Thread.currentThread().isInterrupted()) {
                Event first = queue.poll(100, TimeUnit.MILLISECONDS);
                if (first == null) continue;
                List<Event> batch = new ArrayList<>(100);
                batch.add(first);
                queue.drainTo(batch, 99);
                store.insertBatch(batch);
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            log.debug("writer interrupted, draining remainder before exit");
        }

        // final drain — always runs regardless of how the main loop exits
        List<Event> remaining = new ArrayList<>();
        queue.drainTo(remaining);
        if (!remaining.isEmpty()) {
            log.info("flushing {} remaining events on shutdown", remaining.size());
            try {
                store.insertBatch(remaining);
            } catch (Exception e) {
                log.warn("final drain insert failed", e);
            }
        }

        log.info("event writer stopped");
    }
}
