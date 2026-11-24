using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;

namespace LumaTranslate_WinUI.Services;

public sealed class BackendIpcClient : IAsyncDisposable
{
    private const int MaxFrameBytes = 4 * 1024 * 1024;
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        PropertyNamingPolicy = null,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    private readonly string _shortPipeName;
    private readonly SemaphoreSlim _writeLock = new(1, 1);
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<IpcServerEnvelope>> _pending = new();
    private readonly CancellationTokenSource _cts = new();

    private NamedPipeClientStream? _pipe;
    private Task? _readerLoop;
    private ulong _nextRequestId;

    public event Action<AppStateSnapshotDto>? StateSnapshotReceived;
    public event Action<SubtitleEventDto>? SubtitleUpdated;
    public event Action<WorkerRuntimeStateDto, WorkerRuntimeStateDto>? WorkerStatusUpdated;
    public event Action<string, ulong, StageLatenciesDto>? SpeechLatencyUpdated;
    public event Action<string, string>? PipelineErrorReceived;
    public event Action<bool>? ConnectionStateChanged;

    public BackendIpcClient(string fullPipeName = @"\\.\pipe\LumaTranslate.IPC")
    {
        const string prefix = @"\\.\pipe\";
        _shortPipeName = fullPipeName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? fullPipeName[prefix.Length..]
            : fullPipeName;
    }

    public bool IsConnected => _pipe is { IsConnected: true };

    public void StartBackgroundConnection()
    {
        _readerLoop ??= Task.Run(() => ConnectionSupervisorAsync(_cts.Token));
    }

    private async Task ConnectionSupervisorAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            try
            {
                var client = new NamedPipeClientStream(
                    ".",
                    _shortPipeName,
                    PipeDirection.InOut,
                    PipeOptions.Asynchronous);

                await client.ConnectAsync(2500, ct).ConfigureAwait(false);
                _pipe = client;
                ConnectionStateChanged?.Invoke(true);

                await ReadFramesLoopAsync(client, ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                break;
            }
            catch
            {
                // Retry after brief backoff while backend starts up or restarts.
            }
            finally
            {
                ConnectionStateChanged?.Invoke(false);
                FailAllPending("Mất kết nối IPC với tiến trình nền.");
                if (_pipe is not null)
                {
                    await _pipe.DisposeAsync().ConfigureAwait(false);
                    _pipe = null;
                }
            }

            try
            {
                await Task.Delay(600, ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    private async Task ReadFramesLoopAsync(NamedPipeClientStream stream, CancellationToken ct)
    {
        byte[] lenBuf = new byte[4];
        while (!ct.IsCancellationRequested && stream.IsConnected)
        {
            bool hasPrefix = await ReadExactOrEofAsync(stream, lenBuf, ct).ConfigureAwait(false);
            if (!hasPrefix)
            {
                break;
            }

            uint length = BinaryPrimitives.ReadUInt32LittleEndian(lenBuf);
            if (length == 0 || length > MaxFrameBytes)
            {
                throw new InvalidDataException($"Invalid IPC frame size: {length}");
            }

            byte[] payload = new byte[length];
            bool hasBody = await ReadExactOrEofAsync(stream, payload, ct).ConfigureAwait(false);
            if (!hasBody)
            {
                break;
            }

            IpcServerEnvelope? msg = JsonSerializer.Deserialize<IpcServerEnvelope>(payload, JsonOpts);
            if (msg is null)
            {
                continue;
            }

            DispatchIncomingMessage(msg);
        }
    }

    private void DispatchIncomingMessage(IpcServerEnvelope msg)
    {
        switch (msg.Type)
        {
            case "response":
                if (let_req_id(msg.RequestId, out ulong id) && _pending.TryRemove(id, out var tcs))
                {
                    tcs.TrySetResult(msg);
                }
                if (msg.State is not null)
                {
                    StateSnapshotReceived?.Invoke(msg.State);
                }
                break;

            case "state_snapshot":
                if (msg.State is not null)
                {
                    StateSnapshotReceived?.Invoke(msg.State);
                }
                break;

            case "subtitle_updated":
                if (msg.Subtitle is not null)
                {
                    SubtitleUpdated?.Invoke(msg.Subtitle);
                }
                break;

            case "worker_status":
                if (msg.OcrWorker is not null && msg.TtsWorker is not null)
                {
                    WorkerStatusUpdated?.Invoke(msg.OcrWorker, msg.TtsWorker);
                }
                break;

            case "speech_latency_updated":
                if (msg.RegionId is not null && msg.Sequence.HasValue && msg.Latencies is not null)
                {
                    SpeechLatencyUpdated?.Invoke(msg.RegionId, msg.Sequence.Value, msg.Latencies);
                }
                break;

            case "pipeline_error":
                PipelineErrorReceived?.Invoke(
                    msg.Code ?? "ERROR",
                    msg.MessageVi ?? "Đã xảy ra lỗi trong quá trình xử lý.");
                break;
        }
    }

    private static bool let_req_id(ulong? nullable, out ulong value)
    {
        if (nullable.HasValue)
        {
            value = nullable.Value;
            return true;
        }
        value = 0;
        return false;
    }

    private static async Task<bool> ReadExactOrEofAsync(
        Stream stream,
        byte[] buffer,
        CancellationToken ct)
    {
        int offset = 0;
        while (offset < buffer.Length)
        {
            int read = await stream.ReadAsync(buffer.AsMemory(offset, buffer.Length - offset), ct)
                .ConfigureAwait(false);
            if (read == 0)
            {
                return offset == 0 ? false : throw new EndOfStreamException("Truncated IPC frame");
            }
            offset += read;
        }
        return true;
    }

    public async Task<IpcServerEnvelope> SendRequestAsync(
        Dictionary<string, object?> payload,
        CancellationToken ct = default)
    {
        ulong reqId = Interlocked.Increment(ref _nextRequestId);
        payload["request_id"] = reqId;

        var tcs = new TaskCompletionSource<IpcServerEnvelope>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        _pending[reqId] = tcs;

        try
        {
            NamedPipeClientStream? activePipe = _pipe;
            if (activePipe is null || !activePipe.IsConnected)
            {
                throw new IOException("Chưa kết nối được với tiến trình nền LumaTranslate.Backend.");
            }

            byte[] body = JsonSerializer.SerializeToUtf8Bytes(payload, JsonOpts);
            byte[] frame = new byte[4 + body.Length];
            BinaryPrimitives.WriteUInt32LittleEndian(frame.AsSpan(0, 4), (uint)body.Length);
            Buffer.BlockCopy(body, 0, frame, 4, body.Length);

            await _writeLock.WaitAsync(ct).ConfigureAwait(false);
            try
            {
                await activePipe.WriteAsync(frame, ct).ConfigureAwait(false);
                await activePipe.FlushAsync(ct).ConfigureAwait(false);
            }
            finally
            {
                _writeLock.Release();
            }

            using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct, _cts.Token);
            timeoutCts.CancelAfter(TimeSpan.FromSeconds(65));
            return await tcs.Task.WaitAsync(timeoutCts.Token).ConfigureAwait(false);
        }
        finally
        {
            _pending.TryRemove(reqId, out _);
        }
    }

    public Task<IpcServerEnvelope> GetStateAsync(CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "get_state" }, ct);

    public Task<IpcServerEnvelope> UpsertRegionAsync(CaptureRegionDto region, CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "upsert_region", ["region"] = region }, ct);

    public Task<IpcServerEnvelope> DeleteRegionAsync(string regionId, CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "delete_region", ["region_id"] = regionId }, ct);

    public Task<IpcServerEnvelope> SetRegionEnabledAsync(string regionId, bool enabled, CancellationToken ct = default) =>
        SendRequestAsync(new()
        {
            ["cmd"] = "set_region_enabled",
            ["region_id"] = regionId,
            ["enabled"] = enabled,
        }, ct);

    public Task<IpcServerEnvelope> StartMonitoringAsync(CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "start_monitoring" }, ct);

    public Task<IpcServerEnvelope> StopMonitoringAsync(CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "stop_monitoring" }, ct);

    public Task<IpcServerEnvelope> UpdateSettingsAsync(AppSettingsDto settings, CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "update_settings", ["settings"] = settings }, ct);

    public Task<IpcServerEnvelope> RegisterOverlayWindowAsync(
        ulong hwnd,
        bool captureExcluded,
        int x,
        int y,
        uint width,
        uint height,
        CancellationToken ct = default) =>
        SendRequestAsync(new()
        {
            ["cmd"] = "register_overlay_window",
            ["hwnd"] = hwnd,
            ["capture_excluded"] = captureExcluded,
            ["x"] = x,
            ["y"] = y,
            ["width"] = width,
            ["height"] = height,
        }, ct);

    public Task<IpcServerEnvelope> TestTranslationAsync(
        string provider,
        AppSettingsDto settingsOverride,
        CancellationToken ct = default) =>
        SendRequestAsync(new()
        {
            ["cmd"] = "test_translation",
            ["provider"] = provider,
            ["settings_override"] = settingsOverride,
        }, ct);

    public Task<IpcServerEnvelope> TestTtsAsync(
        string? text = null,
        CancellationToken ct = default) =>
        SendRequestAsync(new()
        {
            ["cmd"] = "test_tts",
            ["text"] = string.IsNullOrWhiteSpace(text) ? null : text,
        }, ct);

    public Task<IpcServerEnvelope> QueryHistoryAsync(
        string? searchQuery,
        int limit = 200,
        int offset = 0,
        CancellationToken ct = default) =>
        SendRequestAsync(new()
        {
            ["cmd"] = "query_history",
            ["search_query"] = string.IsNullOrWhiteSpace(searchQuery) ? null : searchQuery,
            ["limit"] = limit,
            ["offset"] = offset,
        }, ct);

    public Task<IpcServerEnvelope> ClearHistoryAsync(CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "clear_history" }, ct);

    public Task<IpcServerEnvelope> ClearCacheAsync(CancellationToken ct = default) =>
        SendRequestAsync(new() { ["cmd"] = "clear_cache" }, ct);

    public async Task RequestBackendShutdownAsync()
    {
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(2));
            await SendRequestAsync(new() { ["cmd"] = "shutdown" }, cts.Token).ConfigureAwait(false);
        }
        catch
        {
            // Ignore timeout on exit
        }
    }

    private void FailAllPending(string message)
    {
        foreach (var kv in _pending)
        {
            if (_pending.TryRemove(kv.Key, out var tcs))
            {
                tcs.TrySetException(new IOException(message));
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        await _cts.CancelAsync().ConfigureAwait(false);
        if (_readerLoop is not null)
        {
            try
            {
                await _readerLoop.ConfigureAwait(false);
            }
            catch
            {
            }
        }
        if (_pipe is not null)
        {
            await _pipe.DisposeAsync().ConfigureAwait(false);
        }
        _writeLock.Dispose();
        _cts.Dispose();
    }
}
