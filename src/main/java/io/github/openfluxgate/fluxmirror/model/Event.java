package io.github.openfluxgate.fluxmirror.model;

public record Event(long tsMs, String direction, String serverName, byte[] rawBytes) {
}
