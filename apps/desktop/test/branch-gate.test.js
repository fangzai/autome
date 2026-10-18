'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const gate = require('../src/write-gate');
const reads = require('../src/ipc-gate');

test('Git branch discovery is allowed through the read gate', () => {
  assert.equal(reads.validateReadRequest({ method: 'project.branches', params: { project_id: 'p1' } }).ok, true);
});

test('default branch selection accepts a branch name with slash and Unicode', () => {
  assert.equal(gate.validateWriteRequest({ op: 'project.set_default_branch', params: {
    project_id: 'p1', branch: 'feature/中文', member: 'backend',
  } }).ok, true);
});

test('default branch selection requires a project and a branch', () => {
  for (const params of [{}, { project_id: 'p1' }, { project_id: 'p1', branch: '' }, { branch: 'main' }]) {
    assert.equal(gate.validateWriteRequest({ op: 'project.set_default_branch', params }).ok, false);
  }
});

test('member repository identifiers cannot name arbitrary filesystem paths', () => {
  for (const member of ['../backend', 'D:\\other', '/tmp/repo', '']) {
    assert.equal(gate.validateWriteRequest({ op: 'project.set_default_branch', params: {
      project_id: 'p1', branch: 'main', member,
    } }).ok, false);
  }
});
