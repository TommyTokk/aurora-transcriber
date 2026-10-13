import {
  fallbackNotice,
  selectionFromValue,
  selectionValue,
  type AsrSelection,
  type ModelSnapshot,
} from "../lib/transcription";

interface Props {
  model: ModelSnapshot;
  disabled: boolean;
  onSelect: (selection: AsrSelection) => void;
}

/** Shows the requested ASR policy, verified transcribe.cpp device and independent CPU VAD runtime. */
export function AsrDeviceControl({ model, disabled, onSelect }: Props) {
  const gpuDevices = model.devices.filter((device) => device.backend !== "CPU");
  const value = selectionValue(model.selection);
  const selectedMissing =
    model.selection.mode === "device" &&
    !gpuDevices.some((device) => device.id === value);
  const cpuAvailable = model.devices.some((device) => device.backend === "CPU");
  return (
    <fieldset className="asr-device-control" disabled={disabled}>
      <legend>ASR device</legend>
      <label htmlFor="asr-device">Requested device</label>
      <div className="action-row">
        <select
          id="asr-device"
          value={value}
          onChange={(event) => onSelect(selectionFromValue(event.target.value))}
          aria-describedby="asr-device-policy"
        >
          <option value="auto">Auto · GPU, then CPU</option>
          {gpuDevices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.backend} · {device.description || device.name} (
              {device.name})
            </option>
          ))}
          {selectedMissing && (
            <option value={value} disabled>
              Selected GPU unavailable
            </option>
          )}
          <option value="cpu" disabled={!cpuAvailable}>
            CPU
          </option>
        </select>
        <button
          className="secondary-button"
          onClick={() => onSelect(model.selection)}
        >
          Reload selection
        </button>
      </div>
      <p role="status">
        {model.phase === "ready" && model.device
          ? `Active ASR (transcribe.cpp): ${model.device.backend} · ${model.device.description || model.device.name} (${model.device.name})`
          : "ASR (transcribe.cpp) device is not active."}
      </p>
      {model.phase === "ready" &&
        model.device?.backend === "CPU" &&
        model.fallbackReason && (
          <p role="status">{fallbackNotice(model.fallbackReason)}</p>
        )}
      <p>VAD: CPU (ONNX)</p>
      <p className="muted" id="asr-device-policy">
        Auto uses Metal on Apple Silicon, CUDA then Vulkan on Windows/Linux. GPU
        choices fall back to CPU if initialization fails. Choosing CPU skips GPU
        attempts. Choices apply for this app session. Every restart returns to
        Auto. Voice detection runs independently on CPU.
      </p>
    </fieldset>
  );
}
