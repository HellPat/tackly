Feature: Your settings
  Your name and picture are seen by the family; your color scheme stays on
  your phone.

  Scenario: Mona renames herself, picks a picture and a color scheme
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    When Mona opens her settings
    And Mona changes her name to "Mo"
    And Mona picks the picture "pets"
    And Mona picks the color scheme "Sunset"
    Then Patrick sees the members Patrick, Mo and Mara
    And Mona's color scheme is "Sunset"
