import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:integration_test/integration_test.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/crypto_box.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/family_sync.dart';
import 'package:uuid/uuid.dart';

// Android emulator: --dart-define=TACKLY_E2E_SERVER=http://10.0.2.2:3000
// Runs against the real Rust server and disposable PostgreSQL database.
const serverUrl = String.fromEnvironment('TACKLY_E2E_SERVER');
const storage = FlutterSecureStorage();

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'live relay: family pairing, offline events, completion, and recovery',
    (tester) async {
      // Each EventStore represents a separate phone. The secure-storage key is
      // cleared between simulated phones because this test runs in one process.
      await storage.delete(key: 'family_credentials_v1');
      final suffix = DateTime.now().microsecondsSinceEpoch;
      const ids = Uuid();
      final ownerStore = await EventStore.open(
        databaseName: 'live_owner_$suffix.db',
        deviceIdOverride: ids.v4(),
      );
      final memberStore = await EventStore.open(
        databaseName: 'live_member_$suffix.db',
        deviceIdOverride: ids.v4(),
      );
      final recoveredStore = await EventStore.open(
        databaseName: 'live_recovered_$suffix.db',
        deviceIdOverride: ids.v4(),
      );
      final ownerSync = FamilySync(ownerStore);
      final memberSync = FamilySync(memberStore);
      final recoveredSync = FamilySync(recoveredStore);
      final owner = AppController(
        ownerStore,
        await ownerStore.load(),
        captureLocation: () async => const CompletionLocation(
          status: 'captured',
          latitude: 52.52,
          longitude: 13.405,
          accuracyMeters: 8,
        ),
      );
      final member = AppController(
        memberStore,
        await memberStore.load(),
        captureLocation: () async =>
            const CompletionLocation(status: 'unavailable'),
      );
      try {
        await ownerSync.createFamily(serverUrl, 'Home');
        final family = ownerSync.credentials!;
        expect(family.owner, isTrue);
        await expectLater(
          ownerSync.createFamily(serverUrl, 'Other'),
          throwsStateError,
        );

        // Local commands work before an upload. The member sees them only
        // after the owner reconnects.
        final listId = await owner.createList('Shopping', '🛒', null);
        await owner.createTask(listId, 'Private milk', '🥛', null);
        final taskId = owner.state.tasks.keys.single;
        expect((await ownerStore.pendingEvents()).length, 2);
        expect(member.state.tasks, isEmpty);

        final invitation = await ownerSync.createInvitation();
        expect(
          FamilyInvitation.fromQr(invitation.qrValue).inviteId,
          invitation.inviteId,
        );
        await storage.delete(key: 'family_credentials_v1');
        final joining = memberSync.joinFamily(
          FamilyInvitation.fromQr(invitation.qrValue),
        );
        String? pending;
        for (var attempt = 0; attempt < 50 && pending == null; attempt++) {
          await Future<void>.delayed(const Duration(milliseconds: 100));
          pending = await ownerSync.pendingJoinDevice(invitation);
        }
        expect(pending, memberStore.deviceId);
        await ownerSync.approveJoin(invitation, pending!);
        await joining.timeout(const Duration(seconds: 20));
        expect(memberSync.credentials!.owner, isFalse);
        await expectLater(memberSync.createInvitation(), throwsStateError);
        await expectLater(memberSync.joinFamily(invitation), throwsStateError);

        await ownerSync.syncOnce();
        await memberSync.syncOnce();
        await member.refreshFromDisk();
        expect(member.state.lists[listId]!.name, 'Shopping');
        expect(member.state.tasks[taskId]!.title, 'Private milk');

        final response = await http.get(
          Uri.parse('$serverUrl/v1/families/${family.familyId}/events?after=0'),
          headers: {'Authorization': 'Bearer ${family.deviceToken}'},
        );
        expect(response.statusCode, 200);
        expect(response.body, isNot(contains('Private milk')));
        expect(response.body, isNot(contains('Shopping')));
        final serverEvents =
            (jsonDecode(response.body) as Map)['events'] as List;
        expect(serverEvents.length, greaterThanOrEqualTo(3));

        // Completion metadata and its reversal also cross the relay as events.
        final completion = await owner.completeTask(taskId);
        await ownerSync.syncOnce();
        await memberSync.syncOnce();
        await member.refreshFromDisk();
        expect(member.state.tasks[taskId]!.isCompleted, isTrue);
        expect(member.state.tasks[taskId]!.completedAtUtc, isNotNull);
        expect(member.state.tasks[taskId]!.latitude, 52.52);
        await owner.revertCompletion(taskId, completion.id);
        await ownerSync.syncOnce();
        await memberSync.syncOnce();
        await member.refreshFromDisk();
        expect(member.state.tasks[taskId]!.isCompleted, isFalse);

        // A recovery code restores the same event history, without changing
        // the recovered member into an owner.
        final memberCredentials = memberSync.credentials!;
        final recovery =
            'tackly1.${family.familyId}.${encodeBytes(family.familyKey)}.${encodeBytes(memberCredentials.recoverySecret)}';
        await storage.delete(key: 'family_credentials_v1');
        await recoveredSync.recoverFamily(serverUrl, recovery);
        expect(recoveredSync.credentials!.owner, isFalse);
        final replayed = await recoveredStore.load();
        expect(replayed.tasks[taskId]!.title, 'Private milk');
        expect(replayed.tasks[taskId]!.isCompleted, isFalse);
      } finally {
        ownerSync.close();
        memberSync.close();
        recoveredSync.close();
        owner.dispose();
        member.dispose();
        await ownerStore.close();
        await memberStore.close();
        await recoveredStore.close();
        await storage.delete(key: 'family_credentials_v1');
      }
    },
    skip: serverUrl.isEmpty,
  );
}
