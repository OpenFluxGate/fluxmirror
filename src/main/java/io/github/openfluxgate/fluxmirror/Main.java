package io.github.openfluxgate.fluxmirror;

import io.github.openfluxgate.fluxmirror.cli.CliArgs;

public class Main {

    public static void main(String[] args) {
        CliArgs cli = CliArgs.parse(args);

        System.err.println("server-name : " + cli.serverName());
        System.err.println("db          : " + cli.dbPath());
        System.err.println("server-cmd  : " + cli.serverCommand());
    }
}
