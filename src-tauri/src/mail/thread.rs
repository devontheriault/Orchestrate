//! Grouping a folder's messages into conversations. Gmail says which thread
//! each message is in, and its word is taken. Everywhere else a message joins
//! the conversation of any message it answers (References and In-Reply-To),
//! and two replies to the same message are one conversation even when that
//! message isn't in the folder. Subjects alone never join anything: two
//! different "Invoice" mails are not one conversation.

use std::collections::HashMap;

use super::{Summary, Thread};

pub fn group(mut messages: Vec<Summary>) -> Vec<Thread> {
    messages.sort_by_key(|m| (m.date, m.uid));
    let mut sets = Sets::new(messages.len());

    // The first message to name each id, or to be it.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut gmail: HashMap<u64, usize> = HashMap::new();
    for (i, m) in messages.iter().enumerate() {
        if let Some(t) = m.gm_thread {
            match gmail.get(&t) {
                Some(&j) => sets.join(i, j),
                None => {
                    gmail.insert(t, i);
                }
            }
            continue;
        }
        let ids = m.message_id.iter().chain(m.references.iter());
        for id in ids {
            match seen.get(id.as_str()) {
                Some(&j) => sets.join(i, j),
                None => {
                    seen.insert(id, i);
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..messages.len() {
        groups.entry(sets.root(i)).or_default().push(i);
    }
    let mut slots: Vec<Option<Summary>> = messages.into_iter().map(Some).collect();
    let mut threads: Vec<Thread> = groups
        .into_values()
        .map(|members| {
            let msgs: Vec<Summary> = members.iter().filter_map(|&i| slots[i].take()).collect();
            let first = &msgs[0];
            let last = &msgs[msgs.len() - 1];
            Thread {
                id: thread_id(first),
                subject: first.subject.clone(),
                unread: msgs.iter().filter(|m| m.unread).count(),
                date: last.date,
                messages: msgs,
            }
        })
        .collect();
    threads.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| b.id.cmp(&a.id)));
    threads
}

/// A conversation's id, the same each time it is listed: Gmail's, or the id
/// of the message that started it, as its oldest message here knows it.
fn thread_id(oldest: &Summary) -> String {
    if let Some(t) = oldest.gm_thread {
        return format!("gm:{t}");
    }
    match oldest.references.first().or(oldest.message_id.as_ref()) {
        Some(id) => format!("m:{id}"),
        None => format!("uid:{}", oldest.uid),
    }
}

/// Disjoint sets over message indexes.
struct Sets(Vec<usize>);

impl Sets {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            // The older message's set absorbs the newer, so roots stay oldest.
            let (keep, other) = if a < b { (a, b) } else { (b, a) };
            self.0[other] = keep;
        }
    }
}
