Feature: A family of three shares its tasks
  Patrick is the head of the family. Mona and Mara join with an invitation
  (QR code or link). Everyone sees the same tasks, live.

  Scenario: Patrick creates a family and connects Mona by QR code and Mara by link
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    When Patrick creates the family "The Smiths"
    And Patrick invites Mona by QR code
    And Patrick invites Mara by link
    Then Patrick, Mona and Mara see the members Patrick, Mona and Mara

  Scenario: New tasks land in Other and everyone sees them
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Patrick adds the tasks "Do the dishes" and "Take out the trash"
    And Mona shows All
    And Mara shows All
    Then Mona and Mara see the tasks "Do the dishes" and "Take out the trash"

  Scenario: The others watch Mona work, and the task is gone once she finishes
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Water the plants"
    And Mona shows All
    And Patrick shows All
    When Mona starts "Water the plants"
    Then Patrick sees that Mona is working on "Water the plants"
    And Patrick cannot tick "Water the plants" off
    When Mona finishes "Water the plants"
    Then Patrick and Mona no longer see "Water the plants"

  Scenario: Ticking a task off, and taking it back with Undo
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Feed the cat"
    And Patrick shows All
    And Mara shows All
    When Mara ticks "Feed the cat" off
    Then Patrick no longer sees "Feed the cat"
    When Mara undoes "Feed the cat done"
    Then Patrick and Mara see the task "Feed the cat"

  Scenario: Patrick gives a task to Mona; it is in her Mine
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Pick up the parcel"
    And Patrick shows All
    When Patrick gives "Pick up the parcel" to Mona
    Then Patrick sees that Mona has "Pick up the parcel"
    When Mona shows Mine
    Then Mona sees the task "Pick up the parcel"
    And Mona sees 1 in Mine

  Scenario: Patrick removes Mara; her phone keeps the past and sees nothing new
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Before"
    And Mara shows All
    And Mara sees the task "Before"
    When Patrick removes Mara from the family
    Then Patrick and Mona see the members Patrick and Mona
    And Patrick and Mona do not see the member Mara
    And Mara's app says she was removed
    When Patrick adds the task "After"
    And Mona shows All
    Then Mona sees the task "After"
    And Mara sees the task "Before"
    And Mara does not see "After"

  Scenario: An AI agent asks for access, Patrick allows it for 30 minutes, then revokes it
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Water the plants"
    When an AI agent asks Patrick's family for access
    Then Patrick sees the agent's code
    When Patrick allows the agent for "30 minutes"
    Then the AI agent is in
    And the AI agent sees the task "Water the plants"
    And Patrick, Mona and Mara see the AI agent with "30 min left"
    When Patrick revokes the AI agent
    Then Patrick, Mona and Mara see no AI agent

  Scenario: Patrick denies an AI agent
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When an AI agent asks Patrick's family for access
    Then Patrick sees the agent's code
    When Patrick denies the agent
    Then the AI agent is turned away
    And Patrick, Mona and Mara see no AI agent

  Scenario: The family opens only after the recovery code is saved
    Given the sync server is running
    And Patrick has opened Tackly
    When Patrick starts creating the family "The Smiths"
    Then Patrick sees a recovery code of 24 words
    And Patrick cannot continue
    When Patrick ticks "I saved it"
    Then Patrick can continue
    When Patrick continues
    Then Patrick sees the main navigation

  Scenario: Places and the members are behind the Family button, my settings behind my picture
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    Then Mona does not see the "Places" button
    When Mona opens the tab "Family"
    Then Mona sees the "Places" button
    And Mona sees the members Patrick, Mona and Mara
    When Mona opens her settings
    Then the field "Your name" of Mona contains "Mona"

  Scenario: Patrick loses his phone and comes back on a new one with the recovery code
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the task "Before the backup"
    And Mona shows All
    And Mona sees the task "Before the backup"
    When Patrick makes a new recovery code
    And Patrick's phone is lost
    And Mona adds the task "While the phone was lost"
    And Pat opens Tackly
    And Pat restores the family with Patrick's recovery code
    And Pat shows All
    Then Pat sees the tasks "Before the backup" and "While the phone was lost"
    And Pat, Mona and Mara see the members Patrick, Mona and Mara
    When Pat adds the task "Back again"
    And Mona shows All
    Then Mona sees the task "Back again"
