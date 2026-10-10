Feature: Typing on the keyboard
  Text is typed one key at a time, the way it is on a phone: the bar reacts to
  each character, and Enter adds.

  Scenario: The Add button appears with the first letter, follows Backspace, and Enter adds
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths"
    Then Patrick does not see the "Add" button
    When Patrick types "Wa" into "Add a task" key by key
    Then the field "Add a task" of Patrick contains "Wa"
    And Patrick sees the "Add" button
    When Patrick presses Backspace 2 times in "Add a task"
    Then the field "Add a task" of Patrick contains ""
    And Patrick does not see the "Add" button
    When Patrick types "Water the plants" into "Add a task" key by key
    And Patrick presses Enter in "Add a task"
    And Patrick shows All
    Then Patrick sees the task "Water the plants"
    And the field "Add a task" of Patrick contains ""
