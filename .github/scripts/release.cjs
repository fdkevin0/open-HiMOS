module.exports = async ({ github, context, core, tag }) => {
  const { repository } = await github.graphql(`
    query($owner: String!, $repo: String!, $tag: String!, $ref: String!) {
      repository(owner: $owner, name: $repo) {
        release(tagName: $tag) { id }
        ref(qualifiedName: $ref) { target { oid } }
      }
    }`, { ...context.repo, tag, ref: `refs/tags/${tag}` });
  if (repository.release) {
    core.info(`${tag} already has a release; skipping.`);
    return;
  }
  if (repository.ref) {
    if (repository.ref.target.oid !== context.sha) {
      throw new Error(`${tag} already points elsewhere; refusing to move it.`);
    }
  } else {
    await github.rest.git.createRef({ ...context.repo, ref: `refs/tags/${tag}`, sha: context.sha });
  }
  await github.rest.actions.createWorkflowDispatch({
    ...context.repo, workflow_id: 'release.yml', ref: tag, inputs: { tag }
  });
};
