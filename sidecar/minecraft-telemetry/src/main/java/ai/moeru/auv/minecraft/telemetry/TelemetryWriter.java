package ai.moeru.auv.minecraft.telemetry;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;

public final class TelemetryWriter {
  private static final long MAX_JSONL_BYTES = 128L * 1024L * 1024L;
  private final Path outputPath;

  public TelemetryWriter(Path outputPath) {
    this.outputPath = outputPath;
  }

  public void append(TelemetrySample sample) throws IOException {
    Path parent = outputPath.getParent();
    if (parent != null) {
      Files.createDirectories(parent);
    }
    rotateIfOversized();
    Files.writeString(
      outputPath,
      sample.toJsonLine() + System.lineSeparator(),
      StandardCharsets.UTF_8,
      StandardOpenOption.CREATE,
      StandardOpenOption.APPEND
    );
  }

  private void rotateIfOversized() throws IOException {
    if (!Files.exists(outputPath)) {
      return;
    }
    long size = Files.size(outputPath);
    if (size < MAX_JSONL_BYTES) {
      return;
    }

    Path rotatedPath =
      outputPath.resolveSibling(outputPath.getFileName().toString() + ".prev");
    Files.deleteIfExists(rotatedPath);
    Files.move(outputPath, rotatedPath, StandardCopyOption.REPLACE_EXISTING);
  }
}
