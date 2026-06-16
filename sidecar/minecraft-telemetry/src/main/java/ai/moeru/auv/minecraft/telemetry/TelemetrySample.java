package ai.moeru.auv.minecraft.telemetry;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

public final class TelemetrySample {
  public String spatialFrameId;
  public long worldTick;
  public long monotonicTimestampMs;
  public int viewportWidth;
  public int viewportHeight;
  public double[] viewMatrix = new double[16];
  public double[] projectionMatrix = new double[16];
  public double eyeX;
  public double eyeY;
  public double eyeZ;
  public double yaw;
  public double pitch;
  public Integer raycastBlockX;
  public Integer raycastBlockY;
  public Integer raycastBlockZ;
  public String raycastFace;
  public String raycastBlockId;
  public final List<NearbyBlockSample> nearbyBlocks = new ArrayList<>();
  public final List<InventoryEntrySample> inventorySummary = new ArrayList<>();

  public TelemetrySample copy() {
    TelemetrySample copy = new TelemetrySample();
    copy.spatialFrameId = spatialFrameId;
    copy.worldTick = worldTick;
    copy.monotonicTimestampMs = monotonicTimestampMs;
    copy.viewportWidth = viewportWidth;
    copy.viewportHeight = viewportHeight;
    copy.viewMatrix = viewMatrix.clone();
    copy.projectionMatrix = projectionMatrix.clone();
    copy.eyeX = eyeX;
    copy.eyeY = eyeY;
    copy.eyeZ = eyeZ;
    copy.yaw = yaw;
    copy.pitch = pitch;
    copy.raycastBlockX = raycastBlockX;
    copy.raycastBlockY = raycastBlockY;
    copy.raycastBlockZ = raycastBlockZ;
    copy.raycastFace = raycastFace;
    copy.raycastBlockId = raycastBlockId;
    for (NearbyBlockSample block : nearbyBlocks) {
      NearbyBlockSample blockCopy = new NearbyBlockSample();
      blockCopy.x = block.x;
      blockCopy.y = block.y;
      blockCopy.z = block.z;
      blockCopy.blockId = block.blockId;
      copy.nearbyBlocks.add(blockCopy);
    }
    for (InventoryEntrySample entry : inventorySummary) {
      InventoryEntrySample entryCopy = new InventoryEntrySample();
      entryCopy.itemId = entry.itemId;
      entryCopy.count = entry.count;
      copy.inventorySummary.add(entryCopy);
    }
    return copy;
  }

  public String toJsonLine() {
    StringBuilder builder = new StringBuilder();
    builder.append('{');
    appendString(builder, "spatial_frame_id", spatialFrameId);
    appendLong(builder, "world_tick", worldTick);
    appendLong(builder, "monotonic_timestamp_ms", monotonicTimestampMs);
    builder.append(",\"viewport\":{");
    builder.append("\"width\":").append(viewportWidth).append(',');
    builder.append("\"height\":").append(viewportHeight).append('}');
    appendDoubleArray(builder, "view_matrix", viewMatrix);
    appendDoubleArray(builder, "projection_matrix", projectionMatrix);
    builder.append(",\"player_pose\":{");
    builder.append("\"eye_position\":{");
    builder.append("\"x\":").append(formatDouble(eyeX)).append(',');
    builder.append("\"y\":").append(formatDouble(eyeY)).append(',');
    builder.append("\"z\":").append(formatDouble(eyeZ)).append('}');
    builder.append(",\"yaw\":").append(formatDouble(yaw));
    builder.append(",\"pitch\":").append(formatDouble(pitch)).append('}');
    if (raycastBlockX != null && raycastBlockY != null && raycastBlockZ != null && raycastFace != null && raycastBlockId != null) {
      builder.append(",\"raycast_hit\":{");
      builder.append("\"block_pos\":{");
      builder.append("\"x\":").append(raycastBlockX).append(',');
      builder.append("\"y\":").append(raycastBlockY).append(',');
      builder.append("\"z\":").append(raycastBlockZ).append('}');
      builder.append(",\"face\":");
      appendJsonStringValue(builder, raycastFace);
      builder.append(",\"block_id\":");
      appendJsonStringValue(builder, raycastBlockId);
      builder.append('}');
    } else {
      builder.append(",\"raycast_hit\":null");
    }
    appendNearbyBlocks(builder, nearbyBlocks);
    appendInventorySummary(builder, inventorySummary);
    builder.append('}');
    return builder.toString();
  }

  private static void appendString(StringBuilder builder, String key, String value) {
    builder.append('"').append(key).append("\":");
    appendJsonStringValue(builder, value);
  }

  private static void appendLong(StringBuilder builder, String key, long value) {
    builder.append(',').append('"').append(key).append("\":").append(value);
  }

  private static void appendDoubleArray(StringBuilder builder, String key, double[] values) {
    builder.append(',').append('"').append(key).append("\":[");
    for (int index = 0; index < values.length; index += 1) {
      if (index > 0) {
        builder.append(',');
      }
      builder.append(formatDouble(values[index]));
    }
    builder.append(']');
  }

  private static void appendNearbyBlocks(StringBuilder builder, List<NearbyBlockSample> nearbyBlocks) {
    builder.append(",\"nearby_blocks\":[");
    for (int index = 0; index < nearbyBlocks.size(); index += 1) {
      if (index > 0) {
        builder.append(',');
      }
      NearbyBlockSample block = nearbyBlocks.get(index);
      builder.append('{');
      builder.append("\"block_pos\":{");
      builder.append("\"x\":").append(block.x).append(',');
      builder.append("\"y\":").append(block.y).append(',');
      builder.append("\"z\":").append(block.z).append('}');
      builder.append(",\"block_id\":");
      appendJsonStringValue(builder, block.blockId);
      builder.append('}');
    }
    builder.append(']');
  }

  private static void appendInventorySummary(StringBuilder builder, List<InventoryEntrySample> inventorySummary) {
    builder.append(",\"inventory_summary\":[");
    for (int index = 0; index < inventorySummary.size(); index += 1) {
      if (index > 0) {
        builder.append(',');
      }
      InventoryEntrySample entry = inventorySummary.get(index);
      builder.append('{');
      builder.append("\"item_id\":");
      appendJsonStringValue(builder, entry.itemId);
      builder.append(",\"count\":").append(entry.count);
      builder.append('}');
    }
    builder.append(']');
  }

  private static void appendJsonStringValue(StringBuilder builder, String value) {
    builder.append('"').append(escape(value)).append('"');
  }

  private static String escape(String value) {
    return value.replace("\\", "\\\\").replace("\"", "\\\"");
  }

  private static String formatDouble(double value) {
    return String.format(Locale.ROOT, "%.6f", value);
  }

  public static final class NearbyBlockSample {
    public int x;
    public int y;
    public int z;
    public String blockId;
  }

  public static final class InventoryEntrySample {
    public String itemId;
    public int count;
  }
}
