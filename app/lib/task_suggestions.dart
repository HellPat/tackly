import 'event_store.dart';

/// Search existing tasks locally; the server never sees task names.
List<TaskItem> suggestTasks(AppState state, String input, {int limit = 5}) {
  final query = _normalize(input);
  if (query.length < 2) return [];
  final ranked = <(TaskItem, int)>[];
  for (final task in state.tasks.values) {
    if (task.hasConflict) continue;
    final score = _score(query, _normalize(task.title));
    if (score > 0) ranked.add((task, score));
  }
  ranked.sort((a, b) {
    final byScore = b.$2.compareTo(a.$2);
    if (byScore != 0) return byScore;
    return a.$1.title.compareTo(b.$1.title);
  });
  return ranked.take(limit).map((entry) => entry.$1).toList();
}

String _normalize(String value) =>
    value.trim().toLowerCase().replaceAll(RegExp(r'\s+'), ' ');

int _score(String query, String title) {
  if (query == title) return 1000;
  if (title.startsWith(query)) return 850 - (title.length - query.length);
  if (title.split(' ').any((word) => word.startsWith(query))) return 750;
  if (title.contains(query)) return 650;
  final distance = _editDistance(query, title);
  final threshold = query.length <= 4 ? 1 : (query.length / 3).ceil();
  return distance <= threshold ? 500 - distance * 40 : 0;
}

int _editDistance(String left, String right) {
  var previous = List<int>.generate(right.length + 1, (index) => index);
  for (var i = 1; i <= left.length; i++) {
    final current = List<int>.filled(right.length + 1, 0)..[0] = i;
    for (var j = 1; j <= right.length; j++) {
      final substitution = left.codeUnitAt(i - 1) == right.codeUnitAt(j - 1)
          ? 0
          : 1;
      current[j] = [
        previous[j] + 1,
        current[j - 1] + 1,
        previous[j - 1] + substitution,
      ].reduce((a, b) => a < b ? a : b);
    }
    previous = current;
  }
  return previous.last;
}
