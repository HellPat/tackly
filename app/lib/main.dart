import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';

import 'app_controller.dart';
import 'background_sync.dart';
import 'event_store.dart';
import 'family_page.dart';
import 'family_sync.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    try {
      await initializeBackgroundSync();
    } catch (_) {
      // The foreground retry loop still works if background scheduling fails.
    }
    final store = await EventStore.open();
    final controller = AppController(store, await store.load());
    final familySync = FamilySync(store);
    await familySync.load();
    controller.attachFamilySync(familySync);
    if (familySync.credentials != null) {
      try {
        await scheduleBackgroundSync();
      } catch (_) {
        // The app can still synchronize while open.
      }
    }
    runApp(TacklyApp(controller: controller));
  } catch (error) {
    runApp(
      MaterialApp(
        home: Scaffold(
          body: Center(child: Text('Could not open local tasks: $error')),
        ),
      ),
    );
  }
}

class TacklyApp extends StatefulWidget {
  const TacklyApp({super.key, required this.controller});
  final AppController controller;

  @override
  State<TacklyApp> createState() => _TacklyAppState();
}

class _TacklyAppState extends State<TacklyApp> with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      unawaited(widget.controller.syncNow());
    }
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Tackly',
    debugShowCheckedModeBanner: false,
    theme: ThemeData(
      useMaterial3: true,
      colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xff34813b)),
      scaffoldBackgroundColor: const Color(0xfff8faf6),
      appBarTheme: const AppBarTheme(
        backgroundColor: Color(0xfff8faf6),
        surfaceTintColor: Colors.transparent,
      ),
    ),
    home: ListsPage(controller: widget.controller),
  );
}

class _FamilyAction extends StatelessWidget {
  const _FamilyAction({required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: 'Family',
    icon: const Icon(Icons.group_outlined),
    onPressed: () => Navigator.push<void>(
      context,
      MaterialPageRoute(builder: (_) => FamilyPage(controller: controller)),
    ),
  );
}

class _SyncWarning extends StatelessWidget {
  const _SyncWarning(this.message);
  final String message;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
    child: Text(message),
  );
}

class ListsPage extends StatelessWidget {
  const ListsPage({super.key, required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: controller,
    builder: (context, _) {
      final state = controller.state;
      return Scaffold(
        appBar: AppBar(
          title: const Text('Tackly 🌱'),
          actions: [
            if (controller.familySync != null)
              _FamilyAction(controller: controller),
          ],
          bottom: controller.syncError == null
              ? null
              : PreferredSize(
                  preferredSize: const Size.fromHeight(32),
                  child: _SyncWarning(controller.syncError!),
                ),
        ),
        body: ListView(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 16),
          children: [
            _ListRow(
              name: 'All Tasks',
              emoji: '☷',
              count: state.openTasks().length,
              onTap: () => _openTasks(context, null),
            ),
            for (final list in state.lists.values)
              _ListRow(
                name: list.name,
                emoji: list.emoji,
                photo: list.photo,
                count: state.openTasks(listId: list.id).length,
                onTap: () => _openTasks(context, list.id),
              ),
          ],
        ),
        bottomNavigationBar: _BottomActions(
          label: 'Add list',
          onPressed: () async {
            final id = await Navigator.push<String>(
              context,
              MaterialPageRoute(
                builder: (_) =>
                    _EditorPage(controller: controller, kind: _EditorKind.list),
              ),
            );
            if (context.mounted && id != null) _openTasks(context, id);
          },
        ),
      );
    },
  );

  void _openTasks(BuildContext context, String? listId) => Navigator.push<void>(
    context,
    MaterialPageRoute(
      builder: (_) => TasksPage(controller: controller, listId: listId),
    ),
  );
}

class TasksPage extends StatefulWidget {
  const TasksPage({super.key, required this.controller, required this.listId});
  final AppController controller;
  final String? listId;

  @override
  State<TasksPage> createState() => _TasksPageState();
}

class _TasksPageState extends State<TasksPage> {
  final Set<String> _finishing = {};

  Future<void> _resolve(TaskItem task) async {
    final choice = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Changes need a choice'),
        content: SizedBox(
          width: double.maxFinite,
          child: ListView(
            shrinkWrap: true,
            children: [
              ListTile(
                title: Text('Keep ${task.title}'),
                subtitle: const Text('Current version on this phone'),
                onTap: () => Navigator.pop(context, 'current'),
              ),
              for (final event in task.conflictingEvents)
                ListTile(
                  title: Text(
                    event.type == 'task.updated'
                        ? event.payload['title'] as String
                        : event.type == 'task.completed'
                        ? 'Mark done'
                        : event.type == 'task.conflict_resolved'
                        ? 'Other phone’s choice'
                        : 'Revert completion',
                  ),
                  subtitle: Text(
                    '${event.originDeviceId == widget.controller.familySync?.store.deviceId ? 'This phone' : 'Other phone'} · ${event.occurredAtUtc.toLocal()}',
                  ),
                  onTap: () => Navigator.pop(context, event.id),
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Later'),
          ),
        ],
      ),
    );
    if (choice == null || !mounted) return;
    try {
      await widget.controller.resolveConflict(
        task.id,
        useEventId: choice == 'current' ? null : choice,
      );
    } catch (error) {
      if (mounted) _showError(error);
    }
  }

  Future<void> _finish(TaskItem task) async {
    if (!_finishing.add(task.id)) return;
    setState(() {});
    try {
      final event = await widget.controller.completeTask(task.id);
      if (!mounted) return;
      final messenger = ScaffoldMessenger.of(context);
      messenger.hideCurrentSnackBar();
      messenger.showSnackBar(
        SnackBar(
          behavior: SnackBarBehavior.floating,
          duration: const Duration(seconds: 7),
          content: Row(
            children: [
              const Icon(
                Icons.check_circle,
                size: 19,
                color: Color(0xffc5e5bd),
              ),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  '`${task.title}`',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
              ),
            ],
          ),
          action: SnackBarAction(
            label: '↶ Revert',
            onPressed: () async {
              try {
                await widget.controller.revertCompletion(task.id, event.id);
              } catch (error) {
                if (mounted) _showError(error);
              }
            },
          ),
        ),
      );
    } catch (error) {
      if (mounted) _showError(error);
    } finally {
      if (mounted) setState(() => _finishing.remove(task.id));
    }
  }

  void _showError(Object error) =>
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text('Could not save: $error')));

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: widget.controller,
    builder: (context, _) {
      final state = widget.controller.state;
      final list = widget.listId == null ? null : state.lists[widget.listId];
      final tasks = state.openTasks(listId: widget.listId).toList();
      return Scaffold(
        appBar: AppBar(
          automaticallyImplyLeading: false,
          title: Text(
            list?.name ?? 'All Tasks',
            style: const TextStyle(fontSize: 21, fontWeight: FontWeight.w500),
          ),
          actions: [
            if (widget.controller.familySync != null)
              _FamilyAction(controller: widget.controller),
          ],
          bottom: widget.controller.syncError == null
              ? null
              : PreferredSize(
                  preferredSize: const Size.fromHeight(32),
                  child: _SyncWarning(widget.controller.syncError!),
                ),
        ),
        body: tasks.isEmpty
            ? const Center(child: Text('Nothing to do here right now. 🌱'))
            : ListView.builder(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
                itemCount: tasks.length,
                itemBuilder: (context, index) {
                  final task = tasks[index];
                  return Card(
                    margin: const EdgeInsets.only(bottom: 8),
                    child: Row(
                      children: [
                        Expanded(
                          child: InkWell(
                            borderRadius: BorderRadius.circular(12),
                            onTap: task.hasConflict
                                ? () => _resolve(task)
                                : null,
                            onLongPress: () => task.hasConflict
                                ? _resolve(task)
                                : Navigator.push<void>(
                                    context,
                                    MaterialPageRoute(
                                      builder: (_) => _EditorPage(
                                        controller: widget.controller,
                                        kind: _EditorKind.task,
                                        taskId: task.id,
                                      ),
                                    ),
                                  ),
                            child: Padding(
                              padding: const EdgeInsets.all(10),
                              child: Row(
                                children: [
                                  _Avatar(emoji: task.emoji, photo: task.photo),
                                  const SizedBox(width: 12),
                                  Expanded(child: Text(task.title)),
                                  if (task.hasConflict)
                                    const Icon(
                                      Icons.sync_problem,
                                      color: Colors.deepOrange,
                                    ),
                                ],
                              ),
                            ),
                          ),
                        ),
                        SizedBox(
                          width: 56,
                          height: 56,
                          child: Center(
                            child: _finishing.contains(task.id)
                                ? const SizedBox(
                                    width: 22,
                                    height: 22,
                                    child: CircularProgressIndicator(
                                      strokeWidth: 2,
                                    ),
                                  )
                                : Checkbox(
                                    value: false,
                                    semanticLabel: 'Mark ${task.title} done',
                                    onChanged: task.hasConflict
                                        ? null
                                        : (_) => _finish(task),
                                  ),
                          ),
                        ),
                      ],
                    ),
                  );
                },
              ),
        bottomNavigationBar: _BottomActions(
          label: 'Add task',
          onPressed: () {
            if (state.lists.isEmpty) {
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(content: Text('Create a list first.')),
              );
              return;
            }
            Navigator.push<void>(
              context,
              MaterialPageRoute(
                builder: (_) => _EditorPage(
                  controller: widget.controller,
                  kind: _EditorKind.task,
                  listId: widget.listId,
                ),
              ),
            );
          },
        ),
      );
    },
  );
}

class _ListRow extends StatelessWidget {
  const _ListRow({
    required this.name,
    required this.emoji,
    required this.count,
    required this.onTap,
    this.photo,
  });
  final String name;
  final String emoji;
  final String? photo;
  final int count;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => Card(
    margin: const EdgeInsets.only(bottom: 8),
    child: InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(12),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Row(
          children: [
            _Avatar(emoji: emoji, photo: photo),
            const SizedBox(width: 12),
            Expanded(child: Text(name)),
            CircleAvatar(
              radius: 15,
              backgroundColor: const Color(0xffe5efdf),
              child: Text('$count', style: const TextStyle(fontSize: 12)),
            ),
          ],
        ),
      ),
    ),
  );
}

class _Avatar extends StatelessWidget {
  const _Avatar({required this.emoji, this.photo, this.size = 42});
  final String emoji;
  final String? photo;
  final double size;

  @override
  Widget build(BuildContext context) => Container(
    width: size,
    height: size,
    clipBehavior: Clip.antiAlias,
    decoration: BoxDecoration(
      color: const Color(0xffedf4e9),
      borderRadius: BorderRadius.circular(12),
    ),
    alignment: Alignment.center,
    child: photo == null
        ? Text(emoji, style: const TextStyle(fontSize: 22))
        : Image.memory(
            base64Decode(photo!),
            fit: BoxFit.cover,
            width: size,
            height: size,
          ),
  );
}

class _BottomActions extends StatelessWidget {
  const _BottomActions({required this.label, required this.onPressed});
  final String label;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => SafeArea(
    top: false,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
          child: SizedBox(
            width: double.infinity,
            height: 50,
            child: FilledButton(onPressed: onPressed, child: Text(label)),
          ),
        ),
        const Divider(height: 1),
        const SizedBox(
          height: 54,
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(Icons.fact_check_outlined, size: 22),
              Text('Tasks', style: TextStyle(fontSize: 11)),
            ],
          ),
        ),
      ],
    ),
  );
}

enum _EditorKind { list, task }

class _EditorPage extends StatefulWidget {
  const _EditorPage({
    required this.controller,
    required this.kind,
    this.listId,
    this.taskId,
  });
  final AppController controller;
  final _EditorKind kind;
  final String? listId;
  final String? taskId;

  @override
  State<_EditorPage> createState() => _EditorPageState();
}

class _EditorPageState extends State<_EditorPage> {
  late final TextEditingController _name;
  late String _emoji;
  String? _photo;
  String? _selectedListId;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    final task = widget.taskId == null
        ? null
        : widget.controller.state.tasks[widget.taskId];
    _name = TextEditingController(text: task?.title ?? '');
    _emoji = task?.emoji ?? (widget.kind == _EditorKind.list ? '🗂️' : '📝');
    _photo = task?.photo;
    _selectedListId = task?.listId ?? widget.listId;
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _chooseImage() async {
    final choice = await showModalBottomSheet<String>(
      context: context,
      showDragHandle: true,
      builder: (context) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: const Icon(Icons.emoji_emotions_outlined),
              title: const Text('Pick emoji'),
              onTap: () => Navigator.pop(context, 'emoji'),
            ),
            ListTile(
              leading: const Icon(Icons.camera_alt_outlined),
              title: const Text('Take photo'),
              onTap: () => Navigator.pop(context, 'camera'),
            ),
          ],
        ),
      ),
    );
    if (!mounted) return;
    if (choice == 'emoji') {
      var value = _emoji;
      final chosen = await showDialog<String>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Pick emoji'),
          content: TextFormField(
            initialValue: value,
            autofocus: true,
            textAlign: TextAlign.center,
            style: const TextStyle(fontSize: 28),
            decoration: const InputDecoration(hintText: '🙂'),
            onChanged: (text) => value = text,
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () => Navigator.pop(context, value.trim()),
              child: const Text('Use emoji'),
            ),
          ],
        ),
      );
      if (mounted && chosen != null && chosen.isNotEmpty) {
        setState(() {
          _emoji = chosen;
          _photo = null;
        });
      }
    } else if (choice == 'camera') {
      try {
        final image = await ImagePicker().pickImage(
          source: ImageSource.camera,
          maxWidth: 512,
          maxHeight: 512,
          imageQuality: 75,
          requestFullMetadata: false,
        );
        if (image == null || !mounted) return;
        final bytes = await image.readAsBytes();
        if (bytes.length > 1024 * 1024) {
          throw StateError('The photo is too large. Please try again.');
        }
        if (mounted) setState(() => _photo = base64Encode(bytes));
      } catch (error) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Could not take photo: $error')),
          );
        }
      }
    }
  }

  Future<void> _save() async {
    if (_saving) return;
    final name = _name.text.trim();
    if (name.isEmpty) return;
    if (widget.kind == _EditorKind.task && _selectedListId == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Choose a list for this task.')),
      );
      return;
    }
    setState(() => _saving = true);
    try {
      String? createdListId;
      if (widget.kind == _EditorKind.list) {
        createdListId = await widget.controller.createList(
          name,
          _emoji,
          _photo,
        );
      } else if (widget.taskId != null) {
        await widget.controller.updateTask(
          widget.taskId!,
          name,
          _emoji,
          _photo,
        );
      } else {
        await widget.controller.createTask(
          _selectedListId!,
          name,
          _emoji,
          _photo,
        );
      }
      if (mounted) Navigator.pop(context, createdListId);
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Could not save: $error')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final isList = widget.kind == _EditorKind.list;
    final editing = widget.taskId != null;
    final list = widget.controller.state.lists[_selectedListId];
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        title: Text(
          isList
              ? 'New list'
              : editing
              ? 'Edit task'
              : 'New task',
          style: const TextStyle(fontSize: 21, fontWeight: FontWeight.w500),
        ),
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Card(
            child: Padding(
              padding: const EdgeInsets.all(10),
              child: Row(
                children: [
                  InkWell(
                    onTap: _chooseImage,
                    borderRadius: BorderRadius.circular(12),
                    child: _Avatar(emoji: _emoji, photo: _photo, size: 48),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: TextField(
                      controller: _name,
                      autofocus: true,
                      maxLength: isList ? 40 : 90,
                      decoration: InputDecoration(
                        hintText: isList ? 'List name' : 'Task name',
                        counterText: '',
                        border: InputBorder.none,
                      ),
                      textInputAction: TextInputAction.done,
                      onSubmitted: (_) => _save(),
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (!isList && list != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
              child: Text(
                'In ${list.name}',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
          if (!isList && list == null) ...[
            const SizedBox(height: 18),
            const Text('List'),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              children: [
                for (final option in widget.controller.state.lists.values)
                  ChoiceChip(
                    label: Text(option.name),
                    selected: _selectedListId == option.id,
                    onSelected: (_) =>
                        setState(() => _selectedListId = option.id),
                  ),
              ],
            ),
          ],
        ],
      ),
      bottomNavigationBar: _BottomActions(
        label: isList
            ? 'Create list'
            : editing
            ? 'Save task'
            : 'Add task',
        onPressed: _saving ? null : _save,
      ),
    );
  }
}
