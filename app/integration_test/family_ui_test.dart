import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/family_sync.dart';
import 'package:tackly/main.dart';

const serverUrl = String.fromEnvironment('TACKLY_E2E_SERVER');
const storage = FlutterSecureStorage();

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('first launch, invitation, logout, and recovery', (tester) async {
    await storage.delete(key: 'family_credentials_v1');
    final suffix = DateTime.now().microsecondsSinceEpoch;
    final store = await EventStore.open(databaseName: 'family_ui_$suffix.db');
    final sync = FamilySync(store);
    final controller = AppController(store, await store.load())
      ..attachFamilySync(sync);

    try {
      await tester.pumpWidget(TacklyApp(controller: controller));
      expect(find.text('Create Family'), findsOneWidget);
      expect(find.text('Scan Invitation Code'), findsOneWidget);
      expect(find.text('Restore with recovery key'), findsOneWidget);
      expect(find.text('All Tasks'), findsNothing);

      await tester.tap(find.text('Create Family'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.widgetWithText(TextField, 'Family name'),
        'Our Home',
      );
      await tester.enterText(
        find.widgetWithText(TextField, 'Sync server URL'),
        serverUrl,
      );
      await tester.tap(find.text('Create'));
      await _waitFor(tester, 'Save your recovery key');
      final recovery = tester
          .widget<SelectableText>(find.byType(SelectableText))
          .data!;
      expect(recovery, startsWith('tackly1.'));
      await tester.tap(find.text("I've saved it"));
      await tester.pumpAndSettle();
      expect(find.text('All Tasks'), findsOneWidget);

      await tester.tap(find.byTooltip('Family'));
      await tester.pumpAndSettle();
      expect(find.text('Logout'), findsOneWidget);
      expect(find.text('Create Family'), findsNothing);
      expect(find.text('Create invitation'), findsNothing);
      await tester.pageBack();
      await tester.pumpAndSettle();

      await tester.tap(find.byTooltip('Invite to family'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Create invitation'));
      await _waitFor(tester, 'Cancel invitation');
      expect(find.byType(QrImageView), findsOneWidget);
      await tester.tap(find.text('Cancel invitation'));
      await tester.pumpAndSettle();
      expect(find.byType(QrImageView), findsNothing);
      await tester.pageBack();
      await tester.pumpAndSettle();

      await controller.createList('Shopping', '🛒', null);
      await _waitForSync(tester, controller);
      final oldDeviceId = store.deviceId;
      await tester.tap(find.byTooltip('Family'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Logout'));
      await tester.pumpAndSettle();
      expect(find.textContaining('recovery key'), findsWidgets);
      await tester.tap(find.text('Log out'));
      await tester.pumpAndSettle();
      if (find.text('Create Family').evaluate().isEmpty) {
        final visible = tester.widgetList<Text>(find.byType(Text)).map((w) => w.data);
        fail('Logout did not reach setup. Visible: $visible');
      }
      expect(find.text('Create Family'), findsOneWidget);
      expect(find.text('Scan Invitation Code'), findsOneWidget);
      expect(find.text('Restore with recovery key'), findsOneWidget);
      expect(controller.state.lists, isEmpty);
      expect(await store.readEvents(), isEmpty);
      expect(await FamilyCredentials.load(), isNull);
      expect(store.deviceId, isNot(oldDeviceId));

      await tester.tap(find.text('Restore with recovery key'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.widgetWithText(TextField, 'Sync server URL'),
        serverUrl,
      );
      await tester.enterText(
        find.widgetWithText(TextField, 'Recovery key'),
        recovery,
      );
      await tester.tap(find.text('Restore').last);
      await _waitFor(tester, 'All Tasks');
      expect(sync.credentials!.owner, isTrue);
      expect(controller.state.familyName, 'Our Home');
      expect(controller.state.lists.values.single.name, 'Shopping');
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      sync.close();
      controller.dispose();
      await store.close();
      await storage.delete(key: 'family_credentials_v1');
    }
  }, skip: serverUrl.isEmpty);
}

Future<void> _waitFor(WidgetTester tester, String label) async {
  for (var attempt = 0; attempt < 70; attempt++) {
    await tester.pump(const Duration(milliseconds: 200));
    if (find.text(label).evaluate().isNotEmpty) return;
  }
  final visibleText = tester
      .widgetList<Text>(find.byType(Text))
      .map((w) => w.data);
  fail('Timed out waiting for "$label". Visible text: $visibleText');
}

Future<void> _waitForSync(WidgetTester tester, AppController controller) async {
  for (var attempt = 0; attempt < 70; attempt++) {
    if (await controller.pendingChangeCount() == 0) return;
    await controller.syncNow();
    await tester.pump(const Duration(milliseconds: 200));
  }
  fail('Timed out waiting for event sync');
}
