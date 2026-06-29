package ai.moeru.auv.minecraft.telemetry;

import net.fabricmc.api.ClientModInitializer;

public final class AuvMinecraftTelemetryMod implements ClientModInitializer {
  public static final String MOD_ID = "auv-minecraft-telemetry";

  @Override
  public void onInitializeClient() {
    TelemetryRecorder.start();
  }
}
