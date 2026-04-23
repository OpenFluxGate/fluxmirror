package io.github.openfluxgate.fluxmirror.cli;

import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;

public record CliArgs(String serverName, String dbPath, Path captureC2s, Path captureS2c, List<String> serverCommand) {

    private static final String USAGE =
            "Usage: fluxmirror --server-name <name> --db <path> [--capture-c2s <path>] [--capture-s2c <path>] -- <server command...>";

    public static CliArgs parse(String[] args) {
        if (args.length == 0 || "--help".equals(args[0]) || "-h".equals(args[0])) {
            System.out.println(USAGE);
            System.exit(0);
        }

        String serverName = null;
        String dbPath = null;
        Path captureC2s = null;
        Path captureS2c = null;
        List<String> serverCommand = List.of();

        int i = 0;
        while (i < args.length) {
            switch (args[i]) {
                case "--server-name" -> {
                    if (++i >= args.length) throw new IllegalArgumentException("--server-name requires a value");
                    serverName = args[i];
                }
                case "--db" -> {
                    if (++i >= args.length) throw new IllegalArgumentException("--db requires a value");
                    dbPath = args[i];
                }
                case "--capture-c2s" -> {
                    if (++i >= args.length) throw new IllegalArgumentException("--capture-c2s requires a value");
                    captureC2s = Path.of(args[i]);
                }
                case "--capture-s2c" -> {
                    if (++i >= args.length) throw new IllegalArgumentException("--capture-s2c requires a value");
                    captureS2c = Path.of(args[i]);
                }
                case "--" -> {
                    serverCommand = Arrays.asList(Arrays.copyOfRange(args, i + 1, args.length));
                    i = args.length; // stop outer loop
                }
                default -> throw new IllegalArgumentException("Unknown option: " + args[i] + "\n" + USAGE);
            }
            i++;
        }

        if (serverName == null) throw new IllegalArgumentException("--server-name is required\n" + USAGE);
        if (dbPath == null) throw new IllegalArgumentException("--db is required\n" + USAGE);
        if (serverCommand.isEmpty()) throw new IllegalArgumentException("Server command is required after --\n" + USAGE);

        return new CliArgs(serverName, dbPath, captureC2s, captureS2c, serverCommand);
    }
}
