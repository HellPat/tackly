import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:integration_test/integration_test.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/family_sync.dart';
import 'package:tackly/main.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'encrypted offline edits sync and conflict resolution converges',
    (tester) async {
      final relay = _Relay();
      final suffix = DateTime.now().microsecondsSinceEpoch;
      final ownerStore = await EventStore.open(
        databaseName: 'family_owner_$suffix.db',
        deviceIdOverride: '00000000-0000-4000-8000-000000000001',
      );
      final memberStore = await EventStore.open(
        databaseName: 'family_member_$suffix.db',
        deviceIdOverride: '00000000-0000-4000-8000-000000000002',
      );
      final familyId = '00000000-0000-4000-8000-000000000003';
      final key = List<int>.filled(32, 7);
      final ownerSync = FamilySync(ownerStore, client: relay.client)
        ..credentials = FamilyCredentials(
          serverUrl: 'https://sync.example.test',
          familyId: familyId,
          deviceToken: 'owner-token',
          familyKey: key,
          recoverySecret: List<int>.filled(32, 1),
          owner: true,
        );
      final memberSync = FamilySync(memberStore, client: relay.client)
        ..credentials = FamilyCredentials(
          serverUrl: 'https://sync.example.test',
          familyId: familyId,
          deviceToken: 'member-token',
          familyKey: key,
          recoverySecret: List<int>.filled(32, 2),
          owner: false,
        );
      final owner = AppController(ownerStore, await ownerStore.load());
      final member = AppController(memberStore, await memberStore.load());
      try {
        // Both phones make progress from their local log while disconnected.
        final listId = await owner.createList('Shopping', '🛒', null);
        await owner.createTask(listId, 'Secret Milk', '🥛', null);
        final taskId = owner.state.tasks.keys.single;
        expect(member.state.tasks, isEmpty);

        await ownerSync.syncOnce();
        await memberSync.syncOnce();
        await member.refreshFromDisk();
        expect(member.state.tasks[taskId]!.title, 'Secret Milk');
        expect(jsonEncode(relay.events), isNot(contains('Secret Milk')));

        await owner.updateTask(taskId, 'Milk one', '🥛', null);
        await member.updateTask(taskId, 'Milk two', '🥛', null);
        await ownerSync.syncOnce();
        await memberSync.syncOnce();
        await ownerSync.syncOnce();
        await owner.refreshFromDisk();
        await member.refreshFromDisk();
        expect(owner.state.tasks[taskId]!.hasConflict, isTrue);
        expect(member.state.tasks[taskId]!.hasConflict, isTrue);

        await tester.pumpWidget(TacklyApp(controller: member));
        await tester.tap(find.text('Shopping'));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Milk one'));
        await tester.pumpAndSettle();
        expect(find.text('Changes need a choice'), findsOneWidget);
        await tester.tap(find.text('Keep Milk one'));
        await tester.pumpAndSettle();
        await memberSync.syncOnce();
        await ownerSync.syncOnce();
        await owner.refreshFromDisk();
        await member.refreshFromDisk();
        expect(owner.state.tasks[taskId]!.hasConflict, isFalse);
        expect(member.state.tasks[taskId]!.hasConflict, isFalse);
        expect(owner.state.tasks[taskId]!.title, 'Milk one');
        expect(member.state.tasks[taskId]!.title, 'Milk one');
      } finally {
        ownerSync.close();
        memberSync.close();
        owner.dispose();
        member.dispose();
        await ownerStore.close();
        await memberStore.close();
      }
    },
  );
}

class _Relay {
  final events = <Map<String, dynamic>>[];

  late final http.Client client = MockClient((request) async {
    if (request.method == 'POST') {
      final body = jsonDecode(request.body) as Map<String, dynamic>;
      for (final raw in body['events'] as List<dynamic>) {
        final event = Map<String, dynamic>.from(raw as Map);
        if (events.any(
          (existing) => existing['event_id'] == event['event_id'],
        )) {
          continue;
        }
        events.add({'sequence': events.length + 1, ...event});
      }
      return http.Response(
        jsonEncode({'accepted': body['events'].length}),
        200,
      );
    }
    final after = int.parse(request.url.queryParameters['after'] ?? '0');
    final page = events.where((event) => (event['sequence'] as int) > after);
    return http.Response(jsonEncode({'events': page.take(20).toList()}), 200);
  });
}
