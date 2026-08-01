import 'dart:async';

import 'package:flutter/material.dart';
import 'package:grpc/service_api.dart' as grpc;

import '../clients.dart';
import '../gen/agent/v1/agent_session.pb.dart';
import '../gen/agent/v1/llm_pool.pb.dart';
import '../gen/agent/v1/metrics_proxy.pb.dart';
import '../gen/agent/v1/session_registry.pb.dart';

/// The Agent View: a live transcript of the running loop (from `AgentSessionService`)
/// over a status bar — mode + context from the same stream, GPU pool from
/// `LlmPoolService.Health`, gRPC p50/p99 from `MetricsProxyService`.
///
/// Everything fails soft: a disconnected gateway greys the affected cell rather than
/// blanking the view.
class AgentViewPage extends StatefulWidget {
  const AgentViewPage({super.key, required this.clients});

  final PortalClients clients;

  @override
  State<AgentViewPage> createState() => _AgentViewPageState();
}

class _AgentViewPageState extends State<AgentViewPage> {
  static const _maxLog = 500;
  final _log = <String>[];
  final _scroll = ScrollController();

  // Status-bar state.
  String _mode = '—';
  int _ctxTokens = 0;
  int _ctxWindow = 0;
  int _ctxMessages = 0;
  bool _active = false;
  List<PoolMemberHealth> _pool = [];
  double? _p50;
  double? _p99;
  bool _streamDown = false;

  StreamSubscription<SessionEvent>? _sub;
  Timer? _poolTimer;
  Timer? _metricsTimer;

  // Driving state: the goal input, and the session this portal drives (minted once
  // via SessionRegistry.Open, then reused so follow-up goals continue the conversation).
  final _goalInput = TextEditingController();
  String? _sessionId;
  bool _sending = false;

  @override
  void initState() {
    super.initState();
    _subscribe();
    _poolTimer =
        Timer.periodic(const Duration(seconds: 3), (_) => _pollPool());
    _metricsTimer =
        Timer.periodic(const Duration(seconds: 5), (_) => _pollMetrics());
    _pollPool();
    _pollMetrics();
  }

  @override
  void dispose() {
    _sub?.cancel();
    _poolTimer?.cancel();
    _metricsTimer?.cancel();
    _goalInput.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _subscribe() {
    _sub?.cancel();
    _streamDown = false;
    _sub = widget.clients.session.subscribe(SubscribeRequest()).listen(
      _onEvent,
      onError: (Object e) => setState(() => _streamDown = true),
      onDone: () => setState(() => _streamDown = true),
    );
  }

  /// Submit a goal to the agent and stream the run. Mirrors [_subscribe], but drives
  /// via `Send` (which returns the same `SessionEvent` stream, so [_onEvent] renders
  /// it unchanged). The `(user, session)` identity rides call metadata — the sessions
  /// gateway requires it — with the session minted once via `SessionRegistry.Open` and
  /// reused, so follow-up goals continue the same conversation. Cancelling the stream
  /// (navigating away / a new goal) cancels the in-flight run server-side.
  Future<void> _send(String goal) async {
    final g = goal.trim();
    if (g.isEmpty || _sending) return;
    setState(() => _sending = true);
    try {
      _sessionId ??=
          (await widget.clients.registry.open(OpenRequest(user: 'portal')))
              .sessionId;
    } catch (_) {
      setState(() {
        _sending = false;
        _streamDown = true; // couldn't reach the --serve-sessions gateway
      });
      return;
    }
    final opts = grpc.CallOptions(metadata: {
      'x-agent-user-id': 'portal',
      'x-agent-session-id': _sessionId!,
    });
    _sub?.cancel();
    _goalInput.clear();
    setState(() {
      _streamDown = false;
      _sending = false;
    });
    _sub = widget.clients.session
        .send(GoalRequest(goal: g), options: opts)
        .listen(
          _onEvent,
          onError: (Object e) => setState(() => _streamDown = true),
          onDone: () {}, // the run ended; keep the transcript (session is reusable)
        );
  }

  void _onEvent(SessionEvent ev) {
    setState(() {
      switch (ev.whichKind()) {
        case SessionEvent_Kind.statusSnapshot:
          final s = ev.statusSnapshot;
          _mode = s.currentMode.isEmpty ? '—' : s.currentMode;
          _ctxTokens = s.contextTokens;
          _ctxWindow = s.contextWindow;
          _ctxMessages = s.contextMessages;
          _active = s.active;
          break;
        case SessionEvent_Kind.runStarted:
          _active = true;
          _append('▶ run: ${ev.runStarted.goal}');
          break;
        case SessionEvent_Kind.runFinished:
          _active = false;
          _append('■ run finished (${ev.runFinished.ok ? "ok" : "error"})');
          break;
        case SessionEvent_Kind.iteration:
          _append('— iteration ${ev.iteration.iter}');
          break;
        case SessionEvent_Kind.token:
          _appendInline(ev.token.text);
          break;
        case SessionEvent_Kind.toolStart:
          _append('🔧 ${ev.toolStart.name}(${ev.toolStart.args})');
          break;
        case SessionEvent_Kind.toolResult:
          final r = ev.toolResult;
          _append('   → ${r.name}: ${r.ok ? "ok" : "error"}');
          break;
        case SessionEvent_Kind.modeSwitch:
          _mode = ev.modeSwitch.to;
          _append('↻ mode → ${ev.modeSwitch.to} (${ev.modeSwitch.reason})');
          break;
        case SessionEvent_Kind.contextUpdate:
          final c = ev.contextUpdate;
          _ctxTokens = c.promptTokens;
          _ctxWindow = c.contextWindow;
          _ctxMessages = c.messages;
          break;
        case SessionEvent_Kind.notSet:
          break;
      }
    });
    _autoScroll();
  }

  void _append(String line) {
    _log.add(line);
    _trim();
  }

  // Token deltas continue the current assistant line rather than starting a new one.
  void _appendInline(String text) {
    if (_log.isEmpty || _log.last.startsWith('▶') || _log.last.startsWith('—')) {
      _log.add(text);
    } else {
      _log[_log.length - 1] = _log.last + text;
    }
    _trim();
  }

  void _trim() {
    if (_log.length > _maxLog) {
      _log.removeRange(0, _log.length - _maxLog);
    }
  }

  void _autoScroll() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.jumpTo(_scroll.position.maxScrollExtent);
      }
    });
  }

  Future<void> _pollPool() async {
    try {
      final r = await widget.clients.pool.health(PoolHealthRequest());
      if (mounted) setState(() => _pool = r.members);
    } catch (_) {
      if (mounted) setState(() => _pool = []);
    }
  }

  Future<void> _pollMetrics() async {
    _p50 = await _quantile(0.50);
    _p99 = await _quantile(0.99);
    if (mounted) setState(() {});
  }

  Future<double?> _quantile(double q) async {
    final query =
        'histogram_quantile($q, sum(rate(agent_provider_request_seconds_bucket[5m])) by (le))';
    try {
      final res = await widget.clients.metrics.query(PromQuery(query: query));
      if (res.error.isNotEmpty || res.series.isEmpty) return null;
      final samples = res.series.first.samples;
      if (samples.isEmpty) return null;
      final v = samples.last.value;
      return v.isFinite ? v : null;
    } catch (_) {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Expanded(child: _transcript()),
        const Divider(height: 1),
        _goalBar(),
        const Divider(height: 1),
        _statusBar(),
      ],
    );
  }

  /// The goal input: type a goal, press Enter or Send, and watch it run. Drives the
  /// `--serve-sessions` gateway (arbitrary agent execution — loopback/UDS only).
  Widget _goalBar() {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: _goalInput,
              decoration: const InputDecoration(
                hintText: 'Send a goal to the agent…',
                isDense: true,
                border: OutlineInputBorder(),
              ),
              onSubmitted: _send,
            ),
          ),
          const SizedBox(width: 8),
          FilledButton.icon(
            onPressed: _sending ? null : () => _send(_goalInput.text),
            icon: const Icon(Icons.send, size: 18),
            label: const Text('Send'),
          ),
        ],
      ),
    );
  }

  Widget _transcript() {
    if (_streamDown) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.cloud_off, size: 40),
            const SizedBox(height: 8),
            const Text('Not receiving events.\n'
                'Start the sessions gateway: `agent --serve-sessions`,\n'
                'then send a goal below or Reconnect to observe.'),
            const SizedBox(height: 8),
            FilledButton(onPressed: _subscribe, child: const Text('Reconnect')),
          ],
        ),
      );
    }
    return Container(
      color: Theme.of(context).colorScheme.surfaceContainerLowest,
      padding: const EdgeInsets.all(12),
      child: ListView.builder(
        controller: _scroll,
        itemCount: _log.length,
        itemBuilder: (_, i) => Text(
          _log[i],
          style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
        ),
      ),
    );
  }

  Widget _statusBar() {
    final alive = _pool.where((m) => m.alive).length;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      child: Wrap(
        spacing: 12,
        runSpacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          _cell(Icons.psychology, 'mode', _active ? '$_mode ●' : _mode),
          _cell(Icons.data_usage, 'context',
              '${_fmt(_ctxTokens)} / ${_fmt(_ctxWindow)} · $_ctxMessages msg'),
          _cell(
            Icons.memory,
            'GPU pool',
            _pool.isEmpty ? 'n/a' : '$alive/${_pool.length} alive',
            detail: _pool
                .map((m) => '${m.name}${m.saturated ? "!" : ""}(${m.inFlight})')
                .join(' '),
          ),
          _cell(
            Icons.speed,
            'gRPC',
            _p50 == null && _p99 == null
                ? 'n/a'
                : 'p50 ${_ms(_p50)} · p99 ${_ms(_p99)}',
          ),
        ],
      ),
    );
  }

  Widget _cell(IconData icon, String label, String value, {String? detail}) {
    return Tooltip(
      message: detail ?? label,
      child: Chip(
        avatar: Icon(icon, size: 16),
        label: Text('$label: $value'),
      ),
    );
  }

  static String _fmt(int n) =>
      n >= 1000 ? '${(n / 1000).toStringAsFixed(1)}k' : '$n';

  static String _ms(double? seconds) =>
      seconds == null ? '—' : '${(seconds * 1000).round()}ms';
}
