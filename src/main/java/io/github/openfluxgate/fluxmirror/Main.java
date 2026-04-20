package io.github.openfluxgate.fluxmirror;

import io.github.openfluxgate.fluxmirror.bridge.ChildProcess;
import io.github.openfluxgate.fluxmirror.bridge.StdioBridge;
import io.github.openfluxgate.fluxmirror.cli.CliArgs;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class Main {

    private static final Logger log = LoggerFactory.getLogger(Main.class);

    public static void main(String[] args) throws Exception {
        CliArgs cli = CliArgs.parse(args);

        ChildProcess child = new ChildProcess(cli.serverCommand());
        child.start();

        Runtime.getRuntime().addShutdownHook(new Thread(child::close, "child-shutdown"));

        log.info("spawned pid={}, server-name={}", child.pid(), cli.serverName());

        StdioBridge bridge = new StdioBridge(System.in, System.out, child);
        bridge.run();
    }
}
