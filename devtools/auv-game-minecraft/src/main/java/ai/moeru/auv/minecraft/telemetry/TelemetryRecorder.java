package ai.moeru.auv.minecraft.telemetry;

import java.io.IOException;
import java.nio.FloatBuffer;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderContext;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderEvents;
import net.minecraft.block.BlockState;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.screen.GameMenuScreen;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.network.ClientPlayerEntity;
import net.minecraft.item.ItemStack;
import net.minecraft.registry.Registries;
import net.minecraft.util.hit.BlockHitResult;
import net.minecraft.util.hit.HitResult;
import net.minecraft.util.math.BlockPos;
import net.minecraft.util.math.Direction;
import org.joml.Matrix4f;
import org.lwjgl.BufferUtils;

public final class TelemetryRecorder {
  // NOTICE: nearby-block sampling is bounded by a radius and a per-frame entry
  // budget because the sample is serialized to one JSONL line per rendered frame
  // at client tick rate. A radius-8 cube is 4913 positions, and an unbounded
  // surface set would add hundreds of entries per line. The budget is safe for
  // the seed-cloud consumer because that cloud is built by unioning positions
  // across frames and de-duplicating: per-frame sparsity still aggregates into
  // dense coverage as the player moves. The two lookup consumers
  // (`verify::target_block_id`, `select_reference_frame`) query one position at a
  // time, so they need presence near the player, not completeness.
  private static final int NEARBY_BLOCK_RADIUS = 8;
  private static final int NEARBY_BLOCK_BUDGET = 128;
  private static final Object SAMPLE_LOCK = new Object();
  private static final String TELEMETRY_SESSION_ID = UUID.randomUUID().toString();
  private static volatile boolean started = false;
  private static TelemetrySample latestTickSample;
  private static TelemetryWriter writer;

  private TelemetryRecorder() {}

  public static synchronized void start() {
    if (started) {
      return;
    }
    started = true;
    ClientTickEvents.END_CLIENT_TICK.register(TelemetryRecorder::recordTick);
    WorldRenderEvents.LAST.register(TelemetryRecorder::recordRender);
  }

  private static void recordTick(MinecraftClient client) {
    if (client.player == null || client.world == null || client.getWindow() == null) {
      return;
    }

    TelemetrySample sample = new TelemetrySample();
    sample.spatialFrameId = String.format("frame-%d-%d", client.world.getTime(), System.nanoTime());
    sample.worldTick = client.world.getTime();
    sample.monotonicTimestampMs = System.nanoTime() / 1_000_000L;
    sample.telemetrySessionId = TELEMETRY_SESSION_ID;
    sample.viewportWidth = client.getWindow().getFramebufferWidth();
    sample.viewportHeight = client.getWindow().getFramebufferHeight();
    populatePlayerPose(client.player, sample);
    populateRaycast(client, sample);
    populateNearbyBlocks(client, sample);
    populateInventory(client.player, sample);
    populateScreenState(client, sample);
    populateResourcePacks(client, sample);

    synchronized (SAMPLE_LOCK) {
      latestTickSample = sample;
    }
  }

  private static void recordRender(WorldRenderContext context) {
    MinecraftClient client = MinecraftClient.getInstance();
    if (client.player == null || client.world == null || client.getWindow() == null) {
      return;
    }

    TelemetrySample sample;
    synchronized (SAMPLE_LOCK) {
      if (latestTickSample == null) {
        return;
      }
      sample = latestTickSample;
      latestTickSample = null;
    }

    sample.monotonicTimestampMs = System.nanoTime() / 1_000_000L;
    sample.viewportWidth = client.getWindow().getFramebufferWidth();
    sample.viewportHeight = client.getWindow().getFramebufferHeight();
    copyMatrix(new Matrix4f(context.positionMatrix()), sample.viewMatrix);
    copyMatrix(new Matrix4f(context.projectionMatrix()), sample.projectionMatrix);

    try {
      telemetryWriter(client).append(sample);
    } catch (IOException ignored) {
      // NOTICE(mc1-telemetry-gate): first gate is best-effort append-only sampling; hard failure/reporting can tighten after a real sample path exists.
    }
  }

  private static void populatePlayerPose(ClientPlayerEntity player, TelemetrySample sample) {
    sample.eyeX = player.getX();
    sample.eyeY = player.getEyeY();
    sample.eyeZ = player.getZ();
    sample.yaw = player.getYaw();
    sample.pitch = player.getPitch();
  }

  private static void populateRaycast(MinecraftClient client, TelemetrySample sample) {
    if (client.crosshairTarget == null || client.crosshairTarget.getType() != HitResult.Type.BLOCK) {
      return;
    }

    BlockHitResult hitResult = (BlockHitResult) client.crosshairTarget;
    BlockState blockState = client.world.getBlockState(hitResult.getBlockPos());
    sample.raycastBlockX = hitResult.getBlockPos().getX();
    sample.raycastBlockY = hitResult.getBlockPos().getY();
    sample.raycastBlockZ = hitResult.getBlockPos().getZ();
    sample.raycastFace = hitResult.getSide().asString();
    sample.raycastBlockId = Registries.BLOCK.getId(blockState.getBlock()).toString();
  }

  // Records surface-exposed blocks around the player's eye, nearest first, up to
  // NEARBY_BLOCK_BUDGET.
  //
  // Only surface blocks are recorded — a non-air block with at least one air
  // neighbour. Interior blocks are excluded on purpose: they never render, so a
  // 3DGS seed cloud initialized on them wastes primitives, and the aim-target
  // consumers can only address faces a player could click. Excluding them also
  // removes roughly an order of magnitude of volume from each line.
  //
  // TODO(nearby-blocks-frustum-culling): entries are radius-bounded, not
  // view-bounded, so blocks behind the player are recorded too. Frustum culling
  // is deferred for two reasons: the view/projection matrices only exist in the
  // render phase (recordRender), not here in the tick phase, and "in view" is
  // containment rather than visibility — the occlusion-signal decision described in
  // docs/ai/references/apps/minecraft/2026-07-27-minecraft-3dgs-spatial-memory-lane-handoff.md
  // owns that semantics. Unlocks when that slice picks a visibility signal.
  private static void populateNearbyBlocks(MinecraftClient client, TelemetrySample sample) {
    ClientPlayerEntity player = client.player;
    double eyeX = player.getX();
    double eyeY = player.getEyeY();
    double eyeZ = player.getZ();
    BlockPos eyeBlock = BlockPos.ofFloored(eyeX, eyeY, eyeZ);

    List<ScoredBlock> candidates = new ArrayList<>();
    BlockPos.Mutable cursor = new BlockPos.Mutable();
    for (int offsetX = -NEARBY_BLOCK_RADIUS; offsetX <= NEARBY_BLOCK_RADIUS; offsetX += 1) {
      for (int offsetY = -NEARBY_BLOCK_RADIUS; offsetY <= NEARBY_BLOCK_RADIUS; offsetY += 1) {
        for (int offsetZ = -NEARBY_BLOCK_RADIUS; offsetZ <= NEARBY_BLOCK_RADIUS; offsetZ += 1) {
          cursor.set(eyeBlock.getX() + offsetX, eyeBlock.getY() + offsetY, eyeBlock.getZ() + offsetZ);
          if (client.world.isOutOfHeightLimit(cursor.getY())) {
            continue;
          }
          BlockState blockState = client.world.getBlockState(cursor);
          if (blockState.isAir() || !hasAirNeighbour(client, cursor)) {
            continue;
          }
          double centerX = cursor.getX() + 0.5 - eyeX;
          double centerY = cursor.getY() + 0.5 - eyeY;
          double centerZ = cursor.getZ() + 0.5 - eyeZ;
          ScoredBlock candidate = new ScoredBlock();
          candidate.pos = cursor.toImmutable();
          candidate.blockId = Registries.BLOCK.getId(blockState.getBlock()).toString();
          candidate.squaredDistance = centerX * centerX + centerY * centerY + centerZ * centerZ;
          candidates.add(candidate);
        }
      }
    }

    candidates.sort(Comparator.comparingDouble(candidate -> candidate.squaredDistance));
    int recorded = Math.min(candidates.size(), NEARBY_BLOCK_BUDGET);
    for (int index = 0; index < recorded; index += 1) {
      ScoredBlock candidate = candidates.get(index);
      TelemetrySample.NearbyBlockSample blockSample = new TelemetrySample.NearbyBlockSample();
      blockSample.x = candidate.pos.getX();
      blockSample.y = candidate.pos.getY();
      blockSample.z = candidate.pos.getZ();
      blockSample.blockId = candidate.blockId;
      sample.nearbyBlocks.add(blockSample);
    }
  }

  // Surface test: true when any of the six face neighbours is air.
  //
  // NOTICE: only air counts as exposure. A block whose only opening is water,
  // glass, or another non-opaque block reads as interior and is dropped. This
  // keeps the test cheap and its failure mode conservative (under-reporting
  // rather than recording buried geometry). World.getBlockState returns void air
  // for positions outside loaded chunks or the height limit, so chunk-edge
  // neighbours read as exposed rather than throwing.
  private static boolean hasAirNeighbour(MinecraftClient client, BlockPos pos) {
    BlockPos.Mutable neighbour = new BlockPos.Mutable();
    for (Direction direction : Direction.values()) {
      neighbour.set(pos.getX() + direction.getOffsetX(), pos.getY() + direction.getOffsetY(), pos.getZ() + direction.getOffsetZ());
      if (client.world.getBlockState(neighbour).isAir()) {
        return true;
      }
    }
    return false;
  }

  private static void populateInventory(ClientPlayerEntity player, TelemetrySample sample) {
    Map<String, Integer> counts = new HashMap<>();
    for (int slot = 0; slot < player.getInventory().size(); slot += 1) {
      ItemStack stack = player.getInventory().getStack(slot);
      if (stack.isEmpty()) {
        continue;
      }
      String itemId = Registries.ITEM.getId(stack.getItem()).toString();
      counts.merge(itemId, stack.getCount(), Integer::sum);
    }

    for (Map.Entry<String, Integer> entry : counts.entrySet()) {
      TelemetrySample.InventoryEntrySample inventoryEntry = new TelemetrySample.InventoryEntrySample();
      inventoryEntry.itemId = entry.getKey();
      inventoryEntry.count = entry.getValue();
      sample.inventorySummary.add(inventoryEntry);
    }
  }

  private static void populateScreenState(MinecraftClient client, TelemetrySample sample) {
    Screen currentScreen = client.currentScreen;
    if (currentScreen == null) {
      sample.screenState = "in_game";
    } else if (currentScreen instanceof GameMenuScreen) {
      sample.screenState = "menu";
    } else {
      sample.screenState = "loading_or_overlay";
    }
  }

  private static void populateResourcePacks(MinecraftClient client, TelemetrySample sample) {
    sample.resourcePackIds.addAll(client.getResourcePackManager().getEnabledIds());
  }

  private static double[] floatBufferToColumnMajorArray(FloatBuffer buffer) {
    double[] values = new double[16];
    for (int index = 0; index < 16; index += 1) {
      values[index] = buffer.get(index);
    }
    return values;
  }

  private static void copyMatrix(Matrix4f source, double[] destination) {
    FloatBuffer buffer = BufferUtils.createFloatBuffer(16);
    source.get(buffer);
    System.arraycopy(floatBufferToColumnMajorArray(buffer), 0, destination, 0, Math.min(16, destination.length));
  }

  private static TelemetryWriter telemetryWriter(MinecraftClient client) {
    if (writer == null) {
      Path runDir = client.runDirectory.toPath();
      writer = new TelemetryWriter(runDir.resolve("auv").resolve("telemetry.jsonl"));
    }
    return writer;
  }

  // Sort key carrier for nearby-block budgeting. Squared distance is kept so the
  // sort never needs a square root.
  private static final class ScoredBlock {
    private BlockPos pos;
    private String blockId;
    private double squaredDistance;
  }
}
