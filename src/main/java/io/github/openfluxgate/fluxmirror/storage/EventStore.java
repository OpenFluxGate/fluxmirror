package io.github.openfluxgate.fluxmirror.storage;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.github.openfluxgate.fluxmirror.model.Event;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.util.List;

public class EventStore implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(EventStore.class);
    private static final ObjectMapper mapper = new ObjectMapper();

    private final Connection conn;
    private final PreparedStatement insertStmt;

    public EventStore(Path dbPath) throws SQLException {
        try {
            Files.createDirectories(dbPath.getParent());
        } catch (Exception e) {
            throw new SQLException("Failed to create db directory: " + dbPath.getParent(), e);
        }

        conn = DriverManager.getConnection("jdbc:sqlite:" + dbPath);
        try (var stmt = conn.createStatement()) {
            stmt.execute("PRAGMA journal_mode = WAL");
            stmt.execute("PRAGMA synchronous = NORMAL");
            stmt.execute("""
                    CREATE TABLE IF NOT EXISTS events (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      ts_ms INTEGER NOT NULL,
                      direction TEXT NOT NULL CHECK (direction IN ('c2s', 's2c')),
                      method TEXT,
                      message_json TEXT NOT NULL,
                      server_name TEXT NOT NULL
                    )""");
            stmt.execute("CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_ms)");
        }
        insertStmt = conn.prepareStatement(
                "INSERT INTO events (ts_ms, direction, method, message_json, server_name) VALUES (?,?,?,?,?)");
        log.info("event store opened: {}", dbPath);
    }

    public void insertBatch(List<Event> events) {
        try {
            conn.setAutoCommit(false);
            for (Event event : events) {
                String messageJson = new String(event.rawBytes(), StandardCharsets.UTF_8);
                String method = extractMethod(event.rawBytes());
                String direction = event.direction();

                insertStmt.setLong(1, event.tsMs());
                insertStmt.setString(2, direction);
                insertStmt.setString(3, method);
                insertStmt.setString(4, messageJson);
                insertStmt.setString(5, event.serverName());
                insertStmt.addBatch();
            }
            insertStmt.executeBatch();
            conn.commit();
        } catch (SQLException e) {
            log.warn("batch insert failed, rolling back {} events: {}", events.size(), e.getMessage());
            try {
                conn.rollback();
            } catch (SQLException re) {
                log.warn("rollback failed: {}", re.getMessage());
            }
        } finally {
            try {
                conn.setAutoCommit(true);
            } catch (SQLException e) {
                log.warn("failed to restore autoCommit: {}", e.getMessage());
            }
        }
    }

    private String extractMethod(byte[] rawBytes) {
        try {
            JsonNode node = mapper.readTree(rawBytes);
            JsonNode method = node.get("method");
            return method != null && method.isTextual() ? method.asText() : null;
        } catch (Exception e) {
            log.debug("failed to parse JSON for method extraction: {}", e.getMessage());
            return null;
        }
    }

    @Override
    public void close() {
        try {
            insertStmt.close();
        } catch (SQLException e) {
            log.warn("failed to close prepared statement: {}", e.getMessage());
        }
        try {
            conn.close();
        } catch (SQLException e) {
            log.warn("failed to close connection: {}", e.getMessage());
        }
        log.info("event store closed");
    }
}
