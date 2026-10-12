// Derived from Rust wire contracts. Run the binding export test to update.

export type Announcement = { id: string, starts_at: string, ends_at: string, title: string, body: string, source_url: string, };

export type AnnouncementFeed = { version: number, updated_at: string, messages: Array<Announcement>, };

export type Change = { request_id: string, expected_revision: number, operation: Operation, id: string | null, content: Content | null, };

export type Content = { title: string, body: string, source_url: string, starts_at: string, ends_at: string, };

export type DashboardState = { actor: string, snapshot: Snapshot,
/**
 * The announcement the answered change created or edited; absent on a plain read.
 */
message_id: string | null, };

export type History = { revision: number, actor: string, at: string, operation: string, id: string | null, content: Content | null, };

export type Message = { id: string, draft: Content, published: Content | null, ever_published: boolean, };

export type Operation = "save" | "publish" | "withdraw";

export type Snapshot = { revision: number, feed_revision: number, exported_revision: number, feed: AnnouncementFeed, messages: Array<Message>, history: Array<History>, };
