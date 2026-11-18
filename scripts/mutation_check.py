#!/usr/bin/env python3
"""Plant one known mistake at a time in an isolated copy and require a named test to fail.

Shipped code and the acceptance contract are never edited. A mutant that survives
means the named test does not actually guard that rule.
"""
import datetime
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

from verify_gate import snapshot

ROOT=Path(__file__).resolve().parents[1]
STORAGE="crates/raft-storage/src/lib.rs"
# One cargo test (build included) may run this long, and print this much, before it is
# killed. A planted mistake can leave a test spinning and printing ~50 MB/s; an unbounded
# capture exhausted the machine's memory three times (docs/INDEPENDENT_REVIEW_S5.md F6).
CASE_TIMEOUT=480
OUTPUT_CAP=256<<20
CAPPED_RUN=Path(__file__).with_name("capped_run.py")

# (name, file, original text, mutated text, test that must fail, cargo package or None for the workspace)
CASES=[
    # S4 moved the majority into raft-protocol's Voters (docs/MEMBERSHIP.md §13):
    # same names and rules, new anchors. quorum_one now counts one vote too few.
    ("quorum_one","crates/raft-protocol/src/lib.rs","        self.len() / 2 + 1\n","        self.len() / 2\n","majority_follows_the_configuration",None),
    ("unsafe_old_term_commit","crates/raft-core/src/lib.rs","if self.state.term_at(n) == Some(self.state.term) && voters.is_quorum(holders) {","if voters.is_quorum(holders) {","previous_term_entry_requires_current_term_commit",None),
    ("reversed_queue","crates/raft-sim/src/lib.rs","let (key, value) = self.queue.pop_first()?;","let (key, value) = self.queue.pop_last()?;","simulation_seed_is_replayable",None),
    ("saturated_pool_spin","crates/raft-node/src/main.rs","                            Instant::now() + PEER_TIMEOUT,\n                        );","                            Instant::now(),\n                        );","saturated_outbound_pool_does_not_starve_status",None),
    # WAL format 2. The first three are the format 1 defect and its close relatives.
    ("wal_skip_record_header_checksum",STORAGE,
     "    if crc32c(&bytes[..20]) != stored {",
     "    if false && crc32c(&bytes[..20]) != stored {",
     "every_single_bit_flip_fails_closed_unchanged","raft-storage"),
    ("wal_truncate_on_bad_header",STORAGE,
     "            let payload_len = check_header(&header, next_sequence)?;",
     "            let payload_len = match check_header(&header, next_sequence) {\n"
     "                Ok(found) => found,\n"
     "                Err(_) => {\n"
     "                    self.remove_torn_tail(offset, length, next_sequence)?;\n"
     "                    break;\n"
     "                }\n"
     "            };",
     "reproduced_length_corruption_fails_closed_unchanged","raft-storage"),
    ("wal_compare_eof_before_header_checksum",STORAGE,
     "            let payload_len = check_header(&header, next_sequence)?;",
     "            let declared = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;\n"
     "            if declared > 0\n"
     "                && declared <= MAX_RECORD_PAYLOAD as u64\n"
     "                && remaining < HEADER_LEN as u64 + declared + 4\n"
     "            {\n"
     "                self.remove_torn_tail(offset, length, next_sequence)?;\n"
     "                break;\n"
     "            }\n"
     "            let payload_len = check_header(&header, next_sequence)?;",
     "reproduced_length_corruption_fails_closed_unchanged","raft-storage"),
    ("wal_skip_record_checksum",STORAGE,
     "            if crc32c(&checksum_input) != u32::from_le_bytes(checksum) {",
     "            if false && crc32c(&checksum_input) != u32::from_le_bytes(checksum) {",
     "every_single_bit_flip_fails_closed_unchanged","raft-storage"),
    ("wal_accept_any_partial_header",STORAGE,
     "                check_partial_header(&header[..available], next_sequence)?;\n",
     "",
     "zero_or_garbage_tail_rejected","raft-storage"),
    ("wal_truncate_before_copy_is_durable",STORAGE,
     "        preserve_torn_tail(&self.directory, sequence, &tail)?;\n        self.file.set_len(offset)?;",
     "        self.file.set_len(offset)?;\n        preserve_torn_tail(&self.directory, sequence, &tail)?;",
     "torn_tail_copy_failure_blocks_truncation","raft-storage"),
    ("wal_unbounded_torn_copies",STORAGE,
     "    while older.len() >= MAX_TORN_COPIES {",
     "    while false && older.len() >= MAX_TORN_COPIES {",
     "torn_tail_copies_are_idempotent_and_bounded","raft-storage"),
    # Falls back to a fresh start on any file header error instead of refusing.
    ("wal_ignore_file_header",STORAGE,
     "        let start = check_file_header(&header, node_id)?;",
     "        let start = check_file_header(&header, node_id).unwrap_or(PRISTINE);",
     "file_header_corruption_and_identity_mismatch_rejected","raft-storage"),
    ("wal_adopt_any_wal_without_identity",STORAGE,
     "                if !untouched {",
     "                if false && !untouched {",
     "interrupted_initialization_is_completed_or_rejected","raft-storage"),
    ("wal_recreate_missing_wal",STORAGE,
     "                return Err(io::Error::new(\n"
     "                    io::ErrorKind::NotFound,\n"
     "                    \"identity exists but raft.wal is missing; refusing to assume an empty node\",\n"
     "                ));",
     "                write_atomically(&directory, \"raft.wal\", &file_header(node_id, PRISTINE), \"init_before_rename_raft.wal\")?;",
     "existing_identity_without_wal_fails_closed","raft-storage"),
    # S2: snapshots and compaction (docs/SNAPSHOT_COMPACTION.md section 11).
    ("s2_compact_beyond_applied","crates/raft-core/src/lib.rs",
     "        if index != self.last_applied || index <= self.state.base().index {",
     "        if index <= self.state.base().index {",
     "compaction_only_covers_applied_entries","raft-core"),
    # Since S4 one function places every barrier (seal); this puts it last.
    ("s2_reply_before_install_is_durable","crates/raft-core/src/lib.rs",
     "        sealed.push(barrier);\n        sealed.extend(rest);",
     "        sealed.extend(rest);\n        sealed.push(barrier);",
     "follower_installs_only_after_the_snapshot_is_durable","raft-core"),
    ("s2_install_already_covered_snapshot","crates/raft-core/src/lib.rs",
     "                if last_included_index <= self.commit_index {",
     "                if false && last_included_index <= self.commit_index {",
     "a_snapshot_we_already_cover_is_acknowledged_without_change","raft-core"),
    ("s2_no_append_before_base_rule","crates/raft-core/src/lib.rs",
     "                let (check_index, check_term, skip) = if prev_log_index < base.index {",
     "                let (check_index, check_term, skip) = if false && prev_log_index < base.index {",
     "append_entries_that_start_before_the_base_count_as_matching","raft-core"),
    ("s2_chunk_offset_unchecked","crates/raft-core/src/lib.rs",
     "                if receiving.data.len() as u64 != offset {",
     "                if false && receiving.data.len() as u64 != offset {",
     "out_of_order_foreign_and_corrupt_chunks_are_refused","raft-core"),
    ("s2_snapshot_crc_unchecked_on_receive","crates/raft-core/src/lib.rs",
     "                if have != total_len || crc32c(&received.data) != total_crc {",
     "                if have != total_len {",
     "out_of_order_foreign_and_corrupt_chunks_are_refused","raft-core"),
    ("s2_keep_suffix_on_term_mismatch","crates/raft-protocol/src/lib.rs",
     "    let entries = if state.term_at(base.index) == Some(base.term) {",
     "    let entries = if state.term_at(base.index).is_some() {",
     "adopt_snapshot_keeps_suffix_only_when_the_base_matches","raft-protocol"),
    ("s2_wal_rewritten_before_snapshot",STORAGE,
     "            write_atomically(\n                &self.directory,\n                \"snapshot\",\n                &encode_snapshot(snapshot),\n                \"install_before_snapshot_rename\",\n            )?;\n            failpoint(\"install_after_snapshot_before_wal\");\n            self.rewrite_wal(next)",
     "            self.rewrite_wal(next)?;\n            write_atomically(\n                &self.directory,\n                \"snapshot\",\n                &encode_snapshot(snapshot),\n                \"install_before_snapshot_rename\",\n            )?;\n            failpoint(\"install_after_snapshot_before_wal\");\n            Ok(())",
     "real_process_crash_during_snapshot_install_recovers","raft-storage"),
    ("s2_snapshot_file_crc_unchecked",STORAGE,
     "    if crc32c(data) != word(56) {",
     "    if false && crc32c(data) != word(56) {",
     "snapshot_corruption_fails_closed_unchanged","raft-storage"),
    ("s2_accept_wal_newer_than_snapshot",STORAGE,
     "            Some(found) if base.index > found.base.index => {",
     "            Some(found) if false && base.index > found.base.index => {",
     "snapshot_and_wal_consistency_rules","raft-storage"),
    ("s2_older_sequence_executes_again","crates/kv-state/src/lib.rs",
     "            if request.sequence < session.sequence {",
     "            if false && request.sequence < session.sequence {",
     "older_sequence_after_newer_is_rejected_without_effect","kv-state"),
    # S3: ReadIndex (docs/READ_INDEX.md section 10).
    ("s3_answer_without_confirmation","crates/raft-core/src/lib.rs",
     "            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {",
     "            Some(read_index) if last_applied >= read_index => {",
     "replies_to_requests_sent_before_the_read_do_not_confirm_it","raft-core"),
    ("s3_round_not_advanced","crates/raft-core/src/lib.rs",
     "        self.read_round += 1;\n",
     "",
     "replies_to_requests_sent_before_the_read_do_not_confirm_it","raft-core"),
    ("s3_skip_current_term_commit_wait","crates/raft-core/src/lib.rs",
     "        if self.role != Role::Leader || !self.committed_in_term() {",
     "        if self.role != Role::Leader {",
     "read_waits_for_a_committed_entry_of_its_term","raft-core"),
    ("s3_keep_reads_after_losing_leadership","crates/raft-core/src/lib.rs",
     "        for read in self.pending_reads.drain(..) {",
     "        for read in std::iter::empty::<PendingRead>() {",
     "reads_fail_when_leadership_is_lost","raft-core"),
    ("s3_answer_before_applied","crates/raft-core/src/lib.rs",
     "            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {",
     "            Some(read_index) if read.round <= confirmed => {",
     "read_waits_until_the_state_machine_reaches_read_index","raft-core"),
    # S4: membership changes (docs/MEMBERSHIP.md section 15).
    ("s4_no_2015_fix","crates/raft-core/src/lib.rs",
     "        if !self.committed_in_term() {\n            return;\n        }\n        let Some(pending) = self.pending_change.as_ref() else {",
     "        let Some(pending) = self.pending_change.as_ref() else {",
     "the_2015_counterexample_cannot_lose_a_committed_entry","raft-core"),
    ("s4_two_changes_at_once","crates/raft-core/src/lib.rs",
     "        if self.pending_change.is_some() {\n            return effects.push(fail(\"CHANGE_IN_PROGRESS\"));",
     "        if false && self.pending_change.is_some() {\n            return effects.push(fail(\"CHANGE_IN_PROGRESS\"));",
     "change_requests_are_checked_and_one_runs_at_a_time","raft-core"),
    ("s4_change_over_uncommitted_configuration","crates/raft-core/src/lib.rs",
     "        if index > self.commit_index {\n            self.pending_change = None;",
     "        if false && index > self.commit_index {\n            self.pending_change = None;",
     "change_requests_are_checked_and_one_runs_at_a_time","raft-core"),
    # Lesson 08: judging "uncommitted configuration" before the term commit.
    ("s4_judge_uncommitted_before_term_commit","crates/raft-core/src/lib.rs",
     "        if self.pending_change.is_some() {\n            return effects.push(fail(\"CHANGE_IN_PROGRESS\"));",
     "        if self.pending_change.is_some()\n            || self.configuration().is_some_and(|(index, _)| index > self.commit_index)\n        {\n            return effects.push(fail(\"CHANGE_IN_PROGRESS\"));",
     "a_new_leader_waits_for_a_committed_entry_of_its_term","raft-core"),
    ("s4_count_votes_from_outside","crates/raft-core/src/lib.rs",
     "                    if self.voters().is_quorum(self.votes) {\n                        self.become_leader(effects);",
     "                    if self.votes.len() >= self.voters().majority() {\n                        self.become_leader(effects);",
     "only_voters_of_the_configuration_count","raft-core"),
    # A leader outside its configuration adds its own durable copy. (Adding it to
    # `holders` would change nothing: is_quorum counts members only.)
    ("s4_leader_counts_itself_outside_configuration","crates/raft-core/src/lib.rs",
     "if self.state.term_at(n) == Some(self.state.term) && voters.is_quorum(holders) {",
     "if self.state.term_at(n) == Some(self.state.term) && holders.len() + usize::from(!voters.contains(self.id) && self.durable_index >= n) >= voters.majority() {",
     "removing_a_follower_and_then_the_leader","raft-core"),
    ("s4_no_step_down_after_removal","crates/raft-core/src/lib.rs",
     "            let term = self.state.term;\n            self.step_down(term, effects);",
     "            let _ = self.state.term;",
     "removing_a_follower_and_then_the_leader","raft-core"),
    ("s4_removed_node_campaigns","crates/raft-core/src/lib.rs",
     "            Some((index, voters)) => voters.contains(self.id) || self.commit_index < index,",
     "            Some(_) => true,",
     "removing_a_follower_and_then_the_leader","raft-core"),
    ("s4_campaign_without_configuration","crates/raft-core/src/lib.rs",
     "        match self.configuration() {\n            None => false,",
     "        match self.configuration() {\n            None => true,",
     "a_node_without_configuration_never_campaigns","raft-core"),
    ("s4_no_disruption_guard","crates/raft-core/src/lib.rs",
     "        if self.leader_contact && matches!(message.body, Rpc::RequestVote { .. }) {",
     "        if false && self.leader_contact && matches!(message.body, Rpc::RequestVote { .. }) {",
     "a_recent_leader_contact_blocks_votes_without_a_term_change","raft-core"),
    ("s4_add_without_catch_up","crates/raft-core/src/lib.rs",
     "        while self.matches[node as usize] >= *target {",
     "        while self.matches[node as usize] >= *target || node > 0 {",
     "a_new_node_is_found_in_one_round_trip_and_added_after_ten_rounds","raft-core"),
    ("s4_read_index_fixed_majority","crates/raft-core/src/lib.rs",
     "        let needed = voters\n            .majority()\n            .saturating_sub(usize::from(voters.contains(self.id)));",
     "        let needed = 1;",
     "read_index_needs_a_majority_of_the_current_configuration","raft-core"),
    ("s4_snapshot_records_latest_configuration","crates/raft-core/src/lib.rs",
     "        let Some(voters) = self.voters_at(index) else {",
     "        let Some(voters) = self.configuration().map(|(_, voters)| voters) else {",
     "a_snapshot_records_the_configuration_at_its_position","raft-core"),
    ("s4_no_length_hint","crates/raft-core/src/lib.rs",
     "                        .min(last_log_index.saturating_add(1))\n",
     "",
     "a_new_node_is_found_in_one_round_trip_and_added_after_ten_rounds","raft-core"),
    # Lesson 09: a barrier placed before the step's Applies.
    ("s4_apply_after_barrier","crates/raft-core/src/lib.rs",
     "            .partition(|effect| matches!(effect, Effect::Apply(_)));",
     "            .partition(|_| false);",
     "applies_stay_in_order_when_a_barrier_commits_more","raft-core"),
    # Independent review of 2026-11-14 (lessons 11-13).
    # Lesson 11: recovery refuses an interrupted install that also raised the term.
    ("s2_recover_refuses_newer_term_snapshot",STORAGE,
     "                if next.term < found.base.term {\n",
     "                if false && next.term < found.base.term {\n",
     "interrupted_install_from_a_newer_term_raises_the_term","raft-storage"),
    # Lesson 13: the oversize image is serialized again after every event.
    ("s2_oversize_snapshot_serialized_every_event","crates/raft-node/src/main.rs",
     "        if !due || applied < self.oversize_retry_at {",
     "        if !due {",
     "an_oversize_state_is_not_serialized_again_until_the_log_doubles",None),
    # The next two require the SIMULATOR, not a unit test, to notice a mistake:
    # they survive as soon as its workload stops reaching the situation, which
    # no ordinary test run would show (lessons 11 and 12).
    ("sim_must_catch_unconfirmed_read","crates/raft-core/src/lib.rs",
     "            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {",
     "            Some(read_index) if last_applied >= read_index => {",
     "simulation_randomized_invariants","raft-sim"),
    ("sim_must_catch_interrupted_newer_term_install","crates/raft-sim/src/lib.rs",
     "                    if next.term < s.base.term {\n",
     "                    if false && next.term < s.base.term {\n",
     "simulation_randomized_invariants","raft-sim"),
    # S5: group commit, batches and deltas (docs/PERFORMANCE.md section 10).
    ("s5_delta_keeps_only_first_entry","crates/raft-protocol/src/lib.rs",
     "            append: self.entries_from(truncate_from).unwrap_or(&[]).to_vec(),",
     "            append: self.entries_from(truncate_from).unwrap_or(&[]).iter().take(1).cloned().collect(),",
     "a_batch_is_one_barrier_and_one_append_per_follower","raft-core"),
    ("s5_invalid_request_drops_the_rest","crates/raft-core/src/lib.rs",
     "                effects.push(Effect::Reject(request, \"INVALID_ARGUMENT\"));\n            } else if self.role != Role::Leader {\n                effects.push(Effect::Reject(request, \"NOT_LEADER\"));\n            } else {\n                self.state.push(",
     "                effects.push(Effect::Reject(request, \"INVALID_ARGUMENT\"));\n                break;\n            } else if self.role != Role::Leader {\n                effects.push(Effect::Reject(request, \"NOT_LEADER\"));\n            } else {\n                self.state.push(",
     "a_batch_is_one_barrier_and_one_append_per_follower","raft-core"),
    ("s5_no_group_commit","crates/raft-node/src/main.rs",
     "            while inputs.len() < DRAIN {",
     "            while inputs.len() < 1 {",
     "queued_writes_share_one_barrier_and_status_survives_a_large_map",None),
    ("s5_status_embeds_any_map","crates/raft-node/src/main.rs",
     "        let kv = if kv_bytes > STATUS_KV_BYTES {",
     "        let kv = if kv_bytes > usize::MAX {",
     "queued_writes_share_one_barrier_and_status_survives_a_large_map",None),
    ("s5_delta_not_checked_to_connect","crates/raft-storage/src/lib.rs",
     "    if delta.truncate_from <= state.base().index || delta.truncate_from > state.last_index() + 1 {",
     "    if delta.truncate_from <= state.base().index {",
     "invalid_deltas_are_refused_without_writing","raft-storage"),
    ("s5_apply_skips_truncation","crates/raft-protocol/src/lib.rs",
     "        if delta.truncate_from <= self.last_index() {\n            self.truncate_from(delta.truncate_from);",
     "        if false && delta.truncate_from <= self.last_index() {\n            self.truncate_from(delta.truncate_from);",
     "deltas_write_the_same_bytes_as_whole_states","raft-storage"),
    ("s5_whole_log_in_each_barrier","crates/raft-protocol/src/lib.rs",
     "        let truncate_from = self.dirty_from.unwrap_or(self.last_index() + 1);",
     "        let truncate_from = self.dirty_from.map(|_| self.base.index + 1).unwrap_or(self.last_index() + 1);",
     "the_cost_of_a_write_does_not_grow_with_the_log","raft-core"),
    # Must be caught by the simulator itself: a delta that loses entries.
    ("sim_must_catch_partial_delta","crates/raft-protocol/src/lib.rs",
     "            append: self.entries_from(truncate_from).unwrap_or(&[]).to_vec(),",
     "            append: self.entries_from(truncate_from).unwrap_or(&[]).iter().take(1).cloned().collect(),",
     "simulation_randomized_invariants","raft-sim"),
    # S6 (docs/PERFORMANCE.md section 15): S5 code that the S5 review found
    # unguarded. Before S6 every one of these survived the whole test suite.
    # F2: the follower's limit back to the pre-S5 32, in the core or in the envelope check.
    ("s5_follower_limit_still_32","crates/raft-core/src/lib.rs",
     "let valid_entries = entries.len() <= MAX_BATCH_ENTRIES",
     "let valid_entries = entries.len() <= 32",
     "a_follower_accepts_a_full_batch_and_refuses_one_more","raft-core"),
    ("s5_envelope_limit_still_32","crates/raft-protocol/src/lib.rs",
     "&& entries.len() <= MAX_BATCH_ENTRIES",
     "&& entries.len() <= 32",
     "a_follower_accepts_a_full_batch_and_refuses_one_more","raft-core"),
    # F3: three rules of validate_delta, each removed alone.
    ("s5_delta_entry_term_may_decrease",STORAGE,
     "\n            || entry.term < previous","",
     "invalid_deltas_are_refused_without_writing","raft-storage"),
    ("s5_delta_command_unchecked",STORAGE,
     "\n            || !entry.command.valid()","",
     "invalid_deltas_are_refused_without_writing","raft-storage"),
    ("s5_delta_first_term_zero",STORAGE,
     "\n            || entry.term == 0","",
     "invalid_deltas_are_refused_without_writing","raft-storage"),
    # F5: the node proposes what one drain collected as a single batch, however large.
    ("s5_node_never_splits_proposals","crates/raft-node/src/main.rs",
     "                if bytes > MAX_BATCH_BYTES && count > 0 {",
     "                if false {",
     "oversized_queued_writes_are_split_into_bounded_batches","raft-node"),
    # S10, the final review (2026-11-17): checks that a change to the code could
    # remove without any test failing, and the single-voter read fix (lesson 20).
    ("s10_single_voter_read_waits","crates/raft-core/src/lib.rs",
     "        // ever arrive to finish this read, so it is finished here (lesson 20).\n        self.finish_reads(effects);\n",
     "        // ever arrive to finish this read, so it is finished here (lesson 20).\n",
     "a_single_voter_leader_answers_a_read_without_waiting","raft-core"),
    ("s10_snapshot_index_unbounded","crates/raft-protocol/src/lib.rs",
     "            && self.base.index <= MAX_LOG_INDEX\n            && self.base.term > 0",
     "            && self.base.term > 0",
     "indexes_beyond_the_limit_are_refused","raft-protocol"),
    ("s10_envelope_term_order_unchecked","crates/raft-protocol/src/lib.rs",
     "                        && entries.windows(2).all(|p| p[0].term <= p[1].term)\n","\n",
     "append_entries_terms_must_not_decrease","raft-protocol"),
    ("s10_envelope_prev_term_unchecked","crates/raft-protocol/src/lib.rs",
     "                                && e.term >= *prev_log_term\n","\n",
     "append_entries_terms_must_not_decrease","raft-protocol"),
    ("s10_noop_ignores_term",STORAGE,
     "        if delta.term == self.state.term\n            && delta.voted_for",
     "        if true\n            && delta.voted_for",
     "vote_only_and_term_only_deltas_reach_the_disk","raft-storage"),
    ("s10_noop_ignores_vote",STORAGE,
     "            && delta.voted_for == self.state.voted_for\n","",
     "vote_only_and_term_only_deltas_reach_the_disk","raft-storage"),
    ("s10_recovery_skips_validation",STORAGE,
     "    validate_delta(state, &delta)?;\n    state.apply(&delta);",
     "    state.apply(&delta);",
     "a_record_with_valid_checksums_but_invalid_content_is_refused","raft-storage"),
    ("s10_snapshot_cluster_unchecked",STORAGE,
     "        || bytes[8..32] != cluster\n","\n",
     "a_resealed_snapshot_header_with_bad_fields_is_refused","raft-storage"),
    ("s10_delta_base_check_only_zero",STORAGE,
     "if delta.truncate_from <= state.base().index ||",
     "if delta.truncate_from == 0 ||",
     "a_delta_at_or_below_the_snapshot_base_is_refused","raft-storage"),
    ("s10_no_record_size_bound",STORAGE,
     "        if payload.len() > MAX_RECORD_PAYLOAD {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidInput,\n                \"WAL transaction exceeds",
     "        if false {\n            return Err(io::Error::new(\n                io::ErrorKind::InvalidInput,\n                \"WAL transaction exceeds",
     "a_transaction_over_the_size_limit_is_refused_before_writing","raft-storage"),
    ("s10_kv_restore_size_unchecked","crates/kv-state/src/lib.rs",
     "        if bytes.len() > MAX_SNAPSHOT {","        if false {",
     "an_oversize_snapshot_is_refused_even_when_it_parses","kv-state"),
    ("s10_kv_session_operation_unchecked","crates/kv-state/src/lib.rs",
     "            session.operation.valid()\n                && session","            session\n",
     "a_session_with_an_invalid_operation_is_refused","kv-state"),
    ("s10_directory_race_is_an_error",STORAGE,
     "    match fs::create_dir(path) {\n        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}\n        result => result?,\n    }\n",
     "    fs::create_dir(path)?;\n",
     "opens_racing_to_create_the_same_parent_all_succeed","raft-storage"),
    ("s10_snapshot_decode_skips_valid",STORAGE,
     "    if !snapshot.valid() {\n        return Err(invalid(\"snapshot position or membership is invalid\"));",
     "    if false {\n        return Err(invalid(\"snapshot position or membership is invalid\"));",
     "a_resealed_snapshot_header_with_bad_fields_is_refused","raft-storage"),
    ("s10_envelope_append_unbounded","crates/raft-protocol/src/lib.rs",
     "                        && prev_log_index\n                            .checked_add(entries.len() as u64)\n                            .is_some_and(|last| last <= MAX_LOG_INDEX)\n",
     "",
     "indexes_beyond_the_limit_are_refused","raft-protocol"),
    # S12 (2026-11-18): the four checks the S10 review left unguarded.
    ("s12_envelope_from_self_allowed","crates/raft-protocol/src/lib.rs",
     "            && self.from != id\n","",
     "messages_from_the_receiver_itself_or_with_invalid_voters_are_refused","raft-protocol"),
    ("s12_envelope_voters_unchecked","crates/raft-protocol/src/lib.rs",
     "                        && Voters::from_bits(*voters).is_some()\n","",
     "messages_from_the_receiver_itself_or_with_invalid_voters_are_refused","raft-protocol"),
    ("s12_zero_length_frame_allowed","crates/raft-transport/src/lib.rs",
     "    if length == 0 || length > MAX_FRAME {","    if length > MAX_FRAME {",
     "a_zero_length_frame_is_refused","raft-transport"),
    ("s12_snapshot_file_size_unchecked",STORAGE,
     "    if length > (SNAPSHOT_HEADER_LEN + MAX_SNAPSHOT) as u64 {","    if false {",
     "a_snapshot_file_over_the_size_limit_is_refused_before_reading","raft-storage"),
]

def classify(test,stdout,stderr):
    """detected: the named test itself ran and failed. build_error and
    test_not_run mean the test never ran, so they are not detections either;
    survived means it ran and passed with the mistake in place."""
    if re.search(r"^error(\[E\d+\])?: could not compile",stderr,re.MULTILINE):
        return "build_error"
    # The node's trace writes straight to the process's stdout, which libtest's
    # capture does not intercept, so a trace line can split libtest's own
    # "test NAME ... FAILED" line. Accept any of three signals that name the test:
    # the result line, the panic message, or the closing "failures:" list.
    name=rf"(\S+::)?{re.escape(test)}"
    both=stdout+"\n"+stderr
    signals=[rf"^test {name} \.\.\. FAILED$",rf"thread '{name}' (\(\d+\) )?panicked at",rf"^    {name}$"]
    if any(re.search(signal,both,re.MULTILINE) for signal in signals):
        return "detected"
    # A misspelled or removed test name makes cargo run zero tests and succeed;
    # that says nothing about the mutant, so do not call it a survivor.
    if not re.search(rf"^test {name} \.\.\. ",both,re.MULTILINE):
        return "test_not_run"
    return "survived"

def judge(test,returncode,stdout,stderr):
    """(outcome, detected). A run killed for time or output (exit 124 or 125 from
    capped_run.py) did not fail by itself, so whatever its output says it is not a detection."""
    if returncode==124: return "timeout",False
    if returncode==125: return "output_cap",False
    outcome=classify(test,stdout,stderr)
    return outcome,returncode!=0 and outcome=="detected"

def copy_source(work):
    shutil.copytree(ROOT/"crates",work/"crates")
    shutil.copytree(ROOT/"config",work/"config")
    for f in ("Cargo.toml","Cargo.lock","rust-toolchain.toml"):
        if (ROOT/f).exists(): shutil.copy2(ROOT/f,work/f)

def cargo_test(work,test,package):
    # Each copy builds in its own target directory. A shared one let cargo reuse
    # another case's mutated crate: copies of the same workspace get the same
    # metadata hash, and copied files keep their old modification times
    # (docs/lessons/06-mutants-sharing-a-build-directory/).
    env=dict(os.environ)
    env["CARGO_TARGET_DIR"]=str(work/"target")
    scope=["-p",package] if package else ["--workspace"]
    command=["cargo","test",*scope,test]
    capped=[sys.executable,str(CAPPED_RUN),"--timeout",str(CASE_TIMEOUT),"--cap",str(OUTPUT_CAP),"--",*command]
    result=subprocess.run(capped,cwd=work,env=env,capture_output=True,timeout=CASE_TIMEOUT+60)
    (work/"stdout.txt").write_bytes(result.stdout)
    (work/"stderr.txt").write_bytes(result.stderr)
    shutil.rmtree(work/"target",ignore_errors=True)  # created by this run only
    return command,result

def main():
    only=set(sys.argv[1:])
    unknown=only-{name for name,*_ in CASES}
    if unknown: raise SystemExit(f"unknown mutation cases: {sorted(unknown)}")
    cases=[case for case in CASES if not only or case[0] in only]
    # Fail before spending minutes on earlier cases if any anchor has drifted.
    drifted=[name for name,file,original,*_ in cases if (ROOT/file).read_text().count(original)!=1]
    if drifted: raise RuntimeError(f"Mutation anchors missing or not unique: {drifted}")
    _,fingerprint=snapshot(ROOT)
    stamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination=ROOT/"artifacts"/"mutations"/stamp
    destination.mkdir(parents=True)
    # Control: without any mutation, every named test must run and pass;
    # otherwise a later failure would say nothing about the planted mistake.
    controls=[]
    for test,package in sorted({(case[4],case[5] or "") for case in cases}):
        work=destination/"control"/(package or "workspace")/test
        copy_source(work)
        command,result=cargo_test(work,test,package or None)
        passed=result.returncode==0 and re.search(rf"^test (\S+::)?{re.escape(test)} \.\.\. ok$",result.stdout.decode(errors="replace"),re.MULTILINE) is not None
        controls.append({"test":test,"package":package or None,"command":command,"exit_code":result.returncode,"passed":passed})
        print(f"control {test}: {'ok' if passed else 'FAILED'}",flush=True)
    if not all(c["passed"] for c in controls):
        raise SystemExit(f"control runs failed: {[c['test'] for c in controls if not c['passed']]}")
    results=[]
    for name,file,original,mutated,test,package in cases:
        work=destination/name
        copy_source(work)
        path=work/file
        source=path.read_text()
        if source.count(original)!=1: raise RuntimeError(f"Mutation anchor not unique: {name}")
        path.write_text(source.replace(original,mutated,1))
        command,result=cargo_test(work,test,package)
        outcome,detected=judge(test,result.returncode,result.stdout.decode(errors="replace"),result.stderr.decode(errors="replace"))
        results.append({"case":name,"test":test,"command":command,"exit_code":result.returncode,"outcome":outcome,"detected":detected,"directory":str(work.relative_to(ROOT))})
        print(f"{name}: {outcome.upper() if not detected else 'detected'} ({test})",flush=True)
    if snapshot(ROOT)[1]!=fingerprint: raise RuntimeError("source changed while mutants were running")
    undetected=[r["case"] for r in results if not r["detected"]]
    report={"command":"python scripts/mutation_check.py","date":stamp,"source_fingerprint":fingerprint,
            "scope":"isolated source copies; original shipped code and acceptance unchanged",
            "case_count":len(results),"detected_count":len(results)-len(undetected),"undetected":undetected,
            "isolation":"each copy builds in its own CARGO_TARGET_DIR (since 2026-11-13)",
            "control_count":len(controls),"controls":controls,
            "cases":results,"all_detected":not undetected}
    report["partial"]=bool(only)
    text=json.dumps(report,indent=2)+"\n"
    (destination/"report.json").write_text(text)
    if not only:
        # Only a full run may stand as the acceptance report.
        latest=ROOT/"artifacts"/"reports"/"mutations.json"
        latest.parent.mkdir(parents=True,exist_ok=True)
        latest.write_text(text)
    print(json.dumps({k:v for k,v in report.items() if k!="cases"},indent=2))
    return 0 if report["all_detected"] else 1

if __name__=="__main__": raise SystemExit(main())
