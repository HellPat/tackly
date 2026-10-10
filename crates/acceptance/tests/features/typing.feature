Feature: Typing on the keyboard
  Text is typed one key at a time, the way it is on a phone: the form reacts to
  each character, and Enter submits.

  Scenario: Add waits for the first typed letter, follows Backspace, and Enter adds the task
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths"
    When Patrick opens the task list
    Then the "Add" button of Patrick is disabled
    When Patrick types "Wa" into "Add a task" key by key
    Then the field "Add a task" of Patrick contains "Wa"
    And the "Add" button of Patrick is enabled
    When Patrick presses Backspace 2 times in "Add a task"
    Then the field "Add a task" of Patrick contains ""
    And the "Add" button of Patrick is disabled
    When Patrick types "Water the plants" into "Add a task" key by key
    And Patrick presses Enter in "Add a task"
    Then Patrick sees the task "Water the plants"

  Scenario: The bar stays ready after Enter, and finished tasks come back as suggestions
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths"
    When Patrick opens the task list
    And Patrick types "Laundry" into "Add a task" key by key
    And Patrick presses Enter in "Add a task"
    Then the field "Add a task" of Patrick contains ""
    When Patrick types "Dishes" into "Add a task" key by key
    And Patrick presses Enter in "Add a task"
    Then Patrick sees the task "Laundry"
    And Patrick sees the task "Dishes"
    And Patrick does not see the suggestion "Laundry"
    When Patrick finishes "Laundry"
    Then Patrick sees the suggestion "Laundry"
    When Patrick taps the suggestion "Laundry"
    Then Patrick does not see the suggestion "Laundry"
