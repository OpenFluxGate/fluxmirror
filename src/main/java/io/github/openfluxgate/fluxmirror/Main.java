package io.github.openfluxgate.fluxmirror;

import io.github.openfluxgate.fluxmirror.bridge.ChildProcess;
import io.github.openfluxgate.fluxmirror.bridge.StdioBridge;
import io.github.openfluxgate.fluxmirror.cli.CliArgs;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.FileOutputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;

public class Main {

    private static final Logger log = LoggerFactory.getLogger(Main.class);

    public static void main(String[] args) throws Exception {
        System.setOut(new PrintStream(System.out, true, StandardCharsets.UTF_8));
        System.setErr(new PrintStream(System.err, true, StandardCharsets.UTF_8));

        CliArgs cli = CliArgs.parse(args);

        try (OutputStream c2sCap = openIfPresent(cli.captureC2s());
             OutputStream s2cCap = openIfPresent(cli.captureS2c())) {

            ChildProcess child = new ChildProcess(cli.serverCommand());
            child.start();

            Runtime.getRuntime().addShutdownHook(new Thread(child::close, "child-shutdown"));

            log.info("spawned pid={}, server-name={}", child.pid(), cli.serverName());

            if (c2sCap != null) log.info("capturing c2s to {}", cli.captureC2s());
            if (s2cCap != null) log.info("capturing s2c to {}", cli.captureS2c());

            StdioBridge bridge = new StdioBridge(System.in, System.out, child, c2sCap, s2cCap);
            bridge.run();
        }
    }

    private static OutputStream openIfPresent(Path path) throws Exception {
        if (path == null) return null;
        return new FileOutputStream(path.toFile(), true);
    }
}
