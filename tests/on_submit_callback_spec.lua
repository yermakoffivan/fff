---@diagnostic disable: undefined-field, missing-fields
local fff = require('fff')
local fff_rust = require('fff.rust')
local picker_ui = require('fff.picker_ui')
local test_utils = require('tests.utils')

--- Wait until the picker is open with at least one item, then move cursor
--- to the item whose `name` matches `target_name`.
local function wait_for_item(target_name, timeout_ms)
  local found = vim.wait(timeout_ms or 10000, function()
    if not picker_ui.state.active then return false end
    local items = picker_ui.state.filtered_items
    if not items or #items == 0 then return false end
    for _, item in ipairs(items) do
      if item.name == target_name then return true end
    end
    return false
  end, 50)
  if not found then return false end
  for i, item in ipairs(picker_ui.state.filtered_items) do
    if item.name == target_name then
      picker_ui.state.cursor = i
      return true
    end
  end
  return false
end

--- Trigger the picker's `select` keymap (`<CR>`) the same way a user would.
local function press_select()
  local keys = vim.api.nvim_replace_termcodes('<CR>', true, false, true)
  vim.api.nvim_feedkeys(keys, 'x', false)
end

describe('picker on_submit callback (issue #247)', function()
  local sandbox_root, target_dir
  local main_filename = 'fff_target_main.lua'
  local readme_filename = 'README_FIXTURE.md'
  local needle = 'fff_target_unique_needle'

  before_each(function()
    sandbox_root = vim.fn.tempname()
    target_dir = sandbox_root .. '/on-submit-target'
    vim.fn.mkdir(target_dir, 'p')

    local fd = assert(io.open(target_dir .. '/' .. main_filename, 'w'))
    fd:write('-- ' .. needle .. '\nreturn 1\n')
    fd:close()

    fd = assert(io.open(target_dir .. '/' .. readme_filename, 'w'))
    fd:write('docs only — no needle here\n')
    fd:close()

    pcall(vim.api.nvim_del_augroup_by_name, 'fff_file_tracking')
    vim.g.fff = {}
  end)

  after_each(function()
    pcall(picker_ui.close)
    pcall(fff_rust.stop_background_monitor)
    pcall(fff_rust.cleanup_file_picker)
    if sandbox_root then vim.fn.delete(sandbox_root, 'rf') end
    vim.g.fff = nil
  end)

  it('find_files invokes on_submit with the selected item instead of editing', function()
    local pre_buf = vim.api.nvim_buf_get_name(0)

    local captured = {}
    fff.find_files({
      cwd = target_dir,
      on_submit = function(item, ctx)
        captured.item = item
        captured.ctx = ctx
      end,
    })

    assert.is_true(wait_for_item(main_filename, 10000), 'picker never surfaced fixture file')
    press_select()

    local fired = vim.wait(2000, function() return captured.item ~= nil end, 20)
    assert.is_true(fired, 'on_submit was not invoked')

    assert.is_table(captured.ctx)
    assert.are.equal('edit', captured.ctx.action)
    assert.are.equal(main_filename, captured.item.name)
    assert.are.equal(test_utils.normalize(target_dir .. '/' .. main_filename), test_utils.normalize(captured.ctx.path))
    assert.is_nil(captured.ctx.mode, 'find_files mode should be nil')

    -- The callback owns the selection: picker must not have :edit'd a buffer.
    assert.are.equal(pre_buf, vim.api.nvim_buf_get_name(0), ':edit ran despite on_submit being set')
    assert.is_false(picker_ui.state.active)
    assert.is_nil(picker_ui.state.on_submit)
  end)

  it('live_grep invokes on_submit with grep match item and location', function()
    local pre_buf = vim.api.nvim_buf_get_name(0)

    local captured = {}
    fff.live_grep({
      cwd = target_dir,
      query = needle,
      on_submit = function(item, ctx)
        captured.item = item
        captured.ctx = ctx
      end,
    })

    assert.is_true(wait_for_item(main_filename, 10000), 'live_grep never surfaced match')
    press_select()

    local fired = vim.wait(2000, function() return captured.item ~= nil end, 20)
    assert.is_true(fired, 'on_submit was not invoked for live_grep')

    assert.are.equal(main_filename, captured.item.name)
    assert.is_table(captured.ctx.location, 'grep callback should receive a location')
    assert.are.equal(1, captured.ctx.location.line)

    assert.are.equal(pre_buf, vim.api.nvim_buf_get_name(0), ':edit ran despite on_submit being set')
    assert.is_false(picker_ui.state.active)
    assert.is_nil(picker_ui.state.on_submit)
  end)
end)
