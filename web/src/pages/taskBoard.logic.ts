// Pure, dependency-free task-event merge logic for the Task Board live stream.
//
// The gateway replays a bounded window of task events on every (re)connect, so
// the client must dedupe by event id and keep the buffer bounded rather than
// assuming it only ever receives each event once.

export interface TaskEvent {
  id: number | string;
  event_id?: string;
  sequence?: number;
  event_type: string;
  payload: unknown;
  timestamp: string;
}

/** Max events retained in the detail activity feed. */
export const MAX_TASK_EVENTS = 100;

/**
 * Merge incoming events into the existing feed: drop duplicates by event_id or id,
 * sort by sequence or id ascending, and keep only the most recent `cap` events.
 */
export function mergeTaskEvents(
  prev: TaskEvent[],
  incoming: TaskEvent[],
  cap: number = MAX_TASK_EVENTS,
): TaskEvent[] {
  const byKey = new Map<string, TaskEvent>();
  const getKey = (e: TaskEvent) => (e.event_id && e.event_id !== "" ? e.event_id : String(e.id));

  for (const event of prev) byKey.set(getKey(event), event);
  for (const event of incoming) byKey.set(getKey(event), event);

  const merged = [...byKey.values()].sort((a, b) => {
    if (a.sequence !== undefined && b.sequence !== undefined) {
      return a.sequence - b.sequence;
    }
    const numA = typeof a.id === "number" ? a.id : Number(a.id);
    const numB = typeof b.id === "number" ? b.id : Number(b.id);
    if (!isNaN(numA) && !isNaN(numB)) {
      return numA - numB;
    }
    return String(a.id).localeCompare(String(b.id));
  });

  return merged.length > cap ? merged.slice(merged.length - cap) : merged;
}

/** True when an SSE frame carries a task event (vs. an error/comment frame). */
export function isTaskEventFrame(frame: { type?: string }): boolean {
  return frame.type === 'task_event';
}

/** Minimal shape needed to reconstruct a parent-child tree. */
export interface TaskTreeNodeRecord {
  id: string;
  parent_id: string | null;
}

export interface TaskTreeNode<T> {
  record: T;
  children: TaskTreeNode<T>[];
}

/**
 * Rebuild the descendant tree under `rootId` from a flat descendant list (the
 * gateway `/tree` projection). Children are ordered by id for a stable view.
 */
export function buildTaskTree<T extends TaskTreeNodeRecord>(
  rootId: string,
  records: T[],
): TaskTreeNode<T>[] {
  const byParent = new Map<string, T[]>();
  for (const record of records) {
    const key = record.parent_id ?? '';
    const siblings = byParent.get(key);
    if (siblings) siblings.push(record);
    else byParent.set(key, [record]);
  }
  const build = (parentId: string): TaskTreeNode<T>[] =>
    (byParent.get(parentId) ?? [])
      .slice()
      .sort((a, b) => a.id.localeCompare(b.id))
      .map((record) => ({ record, children: build(record.id) }));
  return build(rootId);
}
