const { test } = require('node:test');
const assert = require('node:assert/strict');
const release = require('./release.cjs');

function fixture({ published = false, ref, status } = {}) {
  const calls = [];
  const context = { repo: { owner: 'owner', repo: 'repo' }, sha: 'checked-commit' };
  const github = {
    graphql: async () => {
      if (status) throw Object.assign(new Error('API error'), { status });
      return { repository: { release: published ? { id: 'release' } : null, ref } };
    },
    rest: { git: {
      createRef: async args => calls.push(['tag', args]),
    },
    actions: { createWorkflowDispatch: async args => calls.push(['dispatch', args]) },
  } };
  return { calls, run: () => release({ github, context, core: { info() {} }, tag: 'v0.1.1' }) };
}

test('tags the checked commit and dispatches that exact tag', async () => {
  const f = fixture();
  await f.run();
  assert.deepEqual(f.calls, [
    ['tag', { owner: 'owner', repo: 'repo', ref: 'refs/tags/v0.1.1', sha: 'checked-commit' }],
    ['dispatch', { owner: 'owner', repo: 'repo', workflow_id: 'release.yml', ref: 'v0.1.1', inputs: { tag: 'v0.1.1' } }],
  ]);
});

test('published versions are skipped', async () => {
  const f = fixture({ published: true });
  await f.run();
  assert.deepEqual(f.calls, []);
});

test('retry reuses a matching tag without moving it', async () => {
  const f = fixture({ ref: { target: { oid: 'checked-commit' } } });
  await f.run();
  assert.equal(f.calls.length, 1);
  assert.equal(f.calls[0][0], 'dispatch');
});

test('conflicting tags and API failures stop publication', async () => {
  for (const options of [
    { ref: { target: { oid: 'different-commit' } } },
    { ref: { target: { oid: 'annotated-tag' } } },
    { status: 403 },
    { status: 500 },
  ]) {
    const f = fixture(options);
    await assert.rejects(f.run());
    assert.deepEqual(f.calls, []);
  }
});
