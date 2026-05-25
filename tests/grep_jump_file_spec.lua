---@diagnostic disable: undefined-field
-- Tests for grep mode file-group jump shortcuts (issue #512).
-- Validates grep_jump_to_next_file / grep_jump_to_prev_file move the cursor
-- to the first item of the adjacent file group and trigger page loading
-- when the group boundary lies on the next/previous page.

local picker_ui = require('fff.picker_ui')

local function make_item(path, line, col)
  return { relative_path = path, line_number = line, col = col, line_content = '' }
end

local function reset_state(items, cursor)
  picker_ui.state.active = true
  picker_ui.state.mode = 'grep'
  picker_ui.state.filtered_items = items
  picker_ui.state.items = items
  picker_ui.state.cursor = cursor or 1
  picker_ui.state.pagination = picker_ui.state.pagination or {}
  picker_ui.state.pagination.page_size = #items
  picker_ui.state.pagination.page_index = 0
  picker_ui.state.pagination.total_matched = #items
  picker_ui.state.pagination.grep_file_offsets = { 0 }
  picker_ui.state.pagination.grep_next_file_offset = 0
end

local stubs_installed = false
local function install_stubs()
  if stubs_installed then return end
  picker_ui.render_list = function() end
  picker_ui.update_preview_smart = function() end
  picker_ui.update_preview = function() end
  picker_ui.update_status = function() end
  stubs_installed = true
end

describe('grep_jump_to_next_file / grep_jump_to_prev_file', function()
  before_each(function() install_stubs() end)

  it('jumps to first match of the next file group', function()
    local items = {
      make_item('a.lua', 1, 1),
      make_item('a.lua', 5, 3),
      make_item('a.lua', 9, 1),
      make_item('b.lua', 2, 1),
      make_item('b.lua', 7, 1),
      make_item('c.lua', 4, 2),
    }
    reset_state(items, 1)
    picker_ui.grep_jump_to_next_file()
    assert.are.equal(4, picker_ui.state.cursor)
    picker_ui.grep_jump_to_next_file()
    assert.are.equal(6, picker_ui.state.cursor)
  end)

  it('jumps to first match of the previous file group', function()
    local items = {
      make_item('a.lua', 1, 1),
      make_item('a.lua', 5, 3),
      make_item('b.lua', 2, 1),
      make_item('b.lua', 7, 1),
      make_item('c.lua', 4, 2),
    }
    reset_state(items, 5)
    picker_ui.grep_jump_to_prev_file()
    assert.are.equal(3, picker_ui.state.cursor)
    picker_ui.grep_jump_to_prev_file()
    assert.are.equal(1, picker_ui.state.cursor)
  end)

  it('is a no-op when not in grep mode', function()
    local items = { make_item('a.lua', 1, 1), make_item('b.lua', 1, 1) }
    reset_state(items, 1)
    picker_ui.state.mode = nil
    picker_ui.grep_jump_to_next_file()
    assert.are.equal(1, picker_ui.state.cursor)
  end)

  it('loads next page when no later file group exists on current page', function()
    local page1 = {
      make_item('a.lua', 1, 1),
      make_item('a.lua', 2, 1),
    }
    local page2 = {
      make_item('b.lua', 1, 1),
      make_item('b.lua', 4, 1),
    }
    reset_state(page1, 2)
    -- Pretend more pages are available.
    picker_ui.state.pagination.grep_next_file_offset = 1

    local called = false
    local original_load_next = picker_ui.load_next_page
    picker_ui.load_next_page = function()
      called = true
      picker_ui.state.filtered_items = page2
      picker_ui.state.items = page2
      picker_ui.state.cursor = 1
      picker_ui.state.pagination.page_index = 1
      return true
    end

    picker_ui.grep_jump_to_next_file()
    assert.is_true(called, 'expected load_next_page to be invoked')
    assert.are.equal(1, picker_ui.state.cursor)
    assert.are.equal('b.lua', picker_ui.state.filtered_items[picker_ui.state.cursor].relative_path)

    picker_ui.load_next_page = original_load_next
  end)
end)
