import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:tackly/app_controller.dart';
import 'package:tackly/event_store.dart';
import 'package:tackly/family_sync.dart';
import 'package:tackly/main.dart';
import 'package:uuid/uuid.dart';

const serverUrl = String.fromEnvironment('TACKLY_E2E_SERVER');
const storage = FlutterSecureStorage();

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('family screen creates, invites, cancels, and restores', (
    tester,
  ) async {
    await storage.delete(key: 'family_credentials_v1');
    final suffix = DateTime.now().microsecondsSinceEpoch;
    const ids = Uuid();
    final firstStore = await EventStore.open(
      databaseName: 'family_ui_first_$suffix.db',
      deviceIdOverride: ids.v4(),
    );
    final secondStore = await EventStore.open(
      databaseName: 'family_ui_restore_$suffix.db',
      deviceIdOverride: ids.v4(),
    );
    final firstSync = FamilySync(firstStore);
    final secondSync = FamilySync(secondStore);
    final first = AppController(firstStore, await firstStore.load())
      ..attachFamilySync(firstSync);
    final second = AppController(secondStore, await secondStore.load())
      ..attachFamilySync(secondSync);

    try {
      await tester.pumpWidget(TacklyApp(controller: first));
      await tester.tap(find.byTooltip('Family'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Create family'));
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
      expect(find.text('Invite wife'), findsOneWidget);

      await tester.tap(find.text('Invite wife'));
      await _waitFor(tester, 'Cancel invitation');
      expect(find.byType(QrImageView), findsOneWidget);
      await tester.tap(find.text('Cancel invitation'));
      await tester.pumpAndSettle();
      expect(find.byType(QrImageView), findsNothing);
      await tester.tap(find.text('Sync now'));
      await tester.pumpAndSettle();

      await tester.pumpWidget(const SizedBox.shrink());
      await storage.delete(key: 'family_credentials_v1');
      await tester.pumpWidget(TacklyApp(controller: second));
      await tester.tap(find.byTooltip('Family'));
      await tester.pumpAndSettle();
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
      await _waitFor(tester, 'Family restored. Tasks are syncing.');
      expect(find.text('Family restored. Tasks are syncing.'), findsOneWidget);
      expect(secondSync.credentials!.owner, isTrue);
      expect(second.state.familyName, 'Our Home');
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      firstSync.close();
      secondSync.close();
      first.dispose();
      second.dispose();
      await firstStore.close();
      await secondStore.close();
      await storage.delete(key: 'family_credentials_v1');
    }
  }, skip: serverUrl.isEmpty);
}

Future<void> _waitFor(WidgetTester tester, String label) async {
  for (var attempt = 0; attempt < 50; attempt++) {
    await tester.pump(const Duration(milliseconds: 200));
    if (find.text(label).evaluate().isNotEmpty) return;
  }
  final visibleText = tester
      .widgetList<Text>(find.byType(Text))
      .map((w) => w.data);
  fail('Timed out waiting for "$label". Visible text: $visibleText');
}
