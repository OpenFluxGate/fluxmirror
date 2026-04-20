package io.github.openfluxgate.fluxmirror.cli;

import java.util.Arrays;
import java.util.List;

public record CliArgs(String serverName, String dbPath, List<String> serverCommand) {

    private static final String USAGE =
            "Usage: fluxmirror --server-name <name> --db <path> -- <server command...>";

    public static CliArgs parse(String[] args) {
        String serverName = null;
        String dbPath = null;
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

        return new CliArgs(serverName, dbPath, serverCommand);
    }
}
