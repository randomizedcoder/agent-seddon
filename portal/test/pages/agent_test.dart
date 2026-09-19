import 'package:agent_portal/src/gen/agent/v1/agent_session.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/llm_pool.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/metrics_proxy.pb.dart';
import 'package:agent_portal/src/gen/agent/v1/session_registry.pb.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../testkit/robots/agent_robot.dart';
import 'agent_spec.dart';

/// Layer-A widget tests for the Agent view — iterates [agentSpec]; each row is one
/// test. The trickiest page: a server-streaming `Subscribe` transcript, a
/// `Registry.Open` → `Session.Send` drive with an in-flight guard, `Reconnect`,
/// and two periodic pollers. Each row arranges the four fakes, drives the page via
/// its robot, then asserts the recorded RPC (method + decoded request) and the
/// widget outcome. Timers/streams are drained by the robot's teardown.
void main() {
  PoolHealthReport twoMembers() => PoolHealthReport(members: [
        PoolMemberHealth(name: 'mi50', alive: true, inFlight: 1),
        PoolMemberHealth(name: 'b300', alive: false, saturated: true, inFlight: 0),
      ]);

  PromResult latency(double v) => PromResult(series: [
        PromSeries(samples: [PromSample(value: v)]),
      ]);

  for (final row in agentSpec.rows) {
    testWidgets('agent ${row.label} — ${row.description}', (tester) async {
      final robot = await AgentRobot.create(tester);

      switch (row.label) {
        case 'positive_stream_renders_transcript':
          robot.pool.healthResponse = twoMembers();
          robot.metrics.queryResponse = latency(0.02);
          await robot.load();
          await robot.emit(runStartedEvent('write a poem'));
          await robot.emit(runFinishedEvent(ok: true));
          expect(robot.hasTranscript, isTrue);
          expect(robot.transcriptCount, 2);
          expect(robot.textShown('write a poem'), isTrue);
          expect(robot.log.fired('agent.v1.AgentSessionService/Subscribe'),
              isTrue);

        case 'corner_loading_shows_empty_transcript':
          await robot.pumpLoading();
          // Subscribe is live but silent: no transcript lines, not the down panel.
          expect(robot.hasTranscript, isFalse);
          expect(robot.isStreamDown, isFalse);
          expect(robot.log.fired('agent.v1.AgentSessionService/Subscribe'),
              isTrue);

        case 'boundary_token_deltas_continue_line':
          await robot.load();
          await robot.emit(runStartedEvent('go'));
          await robot.emit(tokenEvent('hel'));
          await robot.emit(tokenEvent('lo'));
          // The run line + one coalesced assistant line ('hello').
          expect(robot.transcriptCount, 2);
          expect(robot.textShown('hello'), isTrue);

        case 'negative_stream_error_shows_reconnect':
          await robot.loadStreamDown();
          expect(robot.isStreamDown, isTrue);
          expect(robot.hasTranscript, isFalse);

        case 'positive_reconnect_resubscribes':
          await robot.loadStreamDown();
          expect(robot.isStreamDown, isTrue);
          await robot.reconnect();
          // Reconnect re-dials Subscribe on a fresh stream.
          expect(
              robot.log.countOf('agent.v1.AgentSessionService/Subscribe'),
              greaterThanOrEqualTo(2));
          // The page clears `_streamDown` on re-subscribe but only repaints on the
          // next event; one arriving on the reconnected stream restores the view.
          await robot.emit(runStartedEvent('resumed'));
          await robot.pumpUntil(() => !robot.isStreamDown,
              reason: 'panel clears after reconnect');
          expect(robot.hasTranscript, isTrue);

        case 'positive_goal_updates_field':
          await robot.load();
          await robot.enterGoal('do the task');
          expect(find.text('do the task').evaluate().isNotEmpty, isTrue);
          // Local only — typing fires no drive RPCs.
          expect(robot.log.fired('agent.v1.SessionRegistryService/Open'),
              isFalse);
          expect(robot.log.fired('agent.v1.AgentSessionService/Send'), isFalse);

        case 'positive_send_opens_then_sends':
          robot.registry.openResponse = OpenResponse(sessionId: 'sess-1');
          await robot.load();
          await robot.enterGoal('refactor the parser');
          await robot.send();
          // Open precedes Send in the recorded order.
          final methods = robot.log.methods;
          final iOpen =
              methods.indexOf('agent.v1.SessionRegistryService/Open');
          final iSend = methods.indexOf('agent.v1.AgentSessionService/Send');
          expect(iOpen, greaterThanOrEqualTo(0));
          expect(iSend, greaterThan(iOpen));
          final open =
              robot.log.last('agent.v1.SessionRegistryService/Open')!.request
                  as OpenRequest;
          expect(open.user, 'portal');
          final sent = robot.log
              .last('agent.v1.AgentSessionService/Send')!
              .request as GoalRequest;
          expect(sent.goal, 'refactor the parser');

        case 'boundary_slow_send_no_double_submit':
          await robot.load();
          await robot.enterGoal('slow goal');
          robot.registry.responseDelay = const Duration(milliseconds: 300);
          // Fire Send; while Open is in flight the button is disabled.
          await robot.tap('agent.send');
          expect(robot.sendEnabled, isFalse);
          // A second tap during the guard window is a no-op (disabled button).
          await robot.tap('agent.send');
          await robot.pumpUntil(
              () => robot.log.fired('agent.v1.AgentSessionService/Send'),
              reason: 'send after slow open');
          // Exactly one Open despite the two taps.
          expect(
              robot.log.countOf('agent.v1.SessionRegistryService/Open'), 1);

        case 'negative_open_error_greys_stream':
          await robot.load();
          await robot.enterGoal('will fail');
          robot.registry.error =
              const GrpcError.unavailable('sessions gateway down');
          await robot.tap('agent.send',
              until: () => robot.isStreamDown);
          // Open was attempted; Send never fired; the button recovered.
          expect(robot.log.fired('agent.v1.SessionRegistryService/Open'),
              isTrue);
          expect(robot.log.fired('agent.v1.AgentSessionService/Send'), isFalse);
          expect(robot.sendEnabled, isTrue);

        case 'adversarial_huge_goal_sent_verbatim':
          await robot.load();
          final huge = 'A' * 200000;
          await robot.enterGoal(huge);
          await robot.send();
          final sent = robot.log
              .last('agent.v1.AgentSessionService/Send')!
              .request as GoalRequest;
          expect(sent.goal.length, 200000);

        case 'positive_pool_poll_renders_and_repolls':
          robot.pool.healthResponse = twoMembers();
          await robot.load();
          expect(robot.exists('agent.status.pool'), isTrue);
          await robot.pumpUntil(() => robot.textShown('1/2 alive'),
              reason: 'pool health rendered');
          // The 3 s periodic poller re-fires Health.
          await robot.advancePoolPoll();
          expect(robot.log.countOf('agent.v1.LlmPoolService/Health'),
              greaterThanOrEqualTo(2));

        case 'negative_pool_error_shows_na':
          robot.pool.error = const GrpcError.unavailable('pool down');
          await robot.load();
          expect(robot.exists('agent.status.pool'), isTrue);
          expect(robot.textShown('GPU pool: n/a'), isTrue);

        case 'positive_grpc_poll_renders_and_repolls':
          robot.metrics.queryResponse = latency(0.05); // 50 ms
          await robot.load();
          expect(robot.exists('agent.status.grpc'), isTrue);
          await robot.pumpUntil(() => robot.textShown('50ms'),
              reason: 'grpc latency rendered');
          // The 5 s periodic poller re-fires Query.
          await robot.advanceMetricsPoll();
          expect(robot.log.countOf('agent.v1.MetricsProxyService/Query'),
              greaterThanOrEqualTo(4));

        case 'boundary_grpc_empty_series_shows_na':
          robot.metrics.queryResponse = PromResult(); // no series
          await robot.load();
          expect(robot.exists('agent.status.grpc'), isTrue);
          expect(robot.textShown('gRPC: n/a'), isTrue);

        default:
          fail('no test body for row ${row.label}');
      }
    });
  }
}
