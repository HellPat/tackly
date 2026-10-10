Feature: Typing on the keyboard
  Text is typed one key at a time, the way it is on a phone: the form reacts to
  each character, and Enter submits.

  Scenario: Add waits for the first typed letter, follows Backspace, and Enter adds the task
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths"
    When Patrick opens the new task form
    Then the "Add" button of Patrick is disabled
    When Patrick types "Wa" into "What needs doing?" key by key
    Then the field "What needs doing?" of Patrick contains "Wa"
    And the "Add" button of Patrick is enabled
    When Patrick presses Backspace 2 times in "What needs doing?"
    Then the field "What needs doing?" of Patrick contains ""
    And the "Add" button of Patrick is disabled
    When Patrick types "Water the plants" into "What needs doing?" key by key
    And Patrick presses Enter in "What needs doing?"
    Then Patrick sees the task "Water the plants"
